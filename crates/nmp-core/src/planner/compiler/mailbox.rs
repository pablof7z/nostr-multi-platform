//! `MailboxCache` trait, `MailboxSnapshot`, and phase-1 implementations.
//!
//! The trait is the seam between the compiler and the `nmp-nip65` crate.
//! Phase 1: `EmptyMailboxCache` + `InMemoryMailboxCache` stubs.
//! Phase 2: replaced by `nmp-nip65::InMemoryMailboxCache`.
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1
//! Doctrine: D3 (outbox routing automatic).

use std::collections::HashMap;
use crate::planner::interest::{Pubkey, RelayUrl};

// ─── MailboxSnapshot ─────────────────────────────────────────────────────────

/// Minimal mailbox snapshot used by the compiler.
///
/// Phase 1: only `write_relays` and `both_relays` are consumed (Outbox
/// direction). Inbox direction (read_relays) is used for `#p` interests.
///
/// Full trait lives in `nmp-nip65::cache::MailboxCache` (later slice).
#[derive(Clone, Debug, Default)]
pub struct MailboxSnapshot {
    pub write_relays: Vec<RelayUrl>,
    pub read_relays: Vec<RelayUrl>,
    pub both_relays: Vec<RelayUrl>,
}

impl MailboxSnapshot {
    /// All relays relevant for Outbox direction (write + both).
    pub fn outbox_relays(&self) -> impl Iterator<Item = &RelayUrl> {
        self.write_relays.iter().chain(self.both_relays.iter())
    }

    /// All relays relevant for Inbox direction (read + both).
    ///
    /// Used for `#p` interests (DMs, notifications) where we want to reach the
    /// tagged pubkey's declared read relays. `both_relays` are included because
    /// the pubkey reads from them too (NIP-65 semantics: `both` = read + write).
    pub fn inbox_relays(&self) -> impl Iterator<Item = &RelayUrl> {
        self.read_relays.iter().chain(self.both_relays.iter())
    }

    /// True iff the snapshot has at least one inbox relay (read or both).
    pub fn has_inbox_relays(&self) -> bool {
        !self.read_relays.is_empty() || !self.both_relays.is_empty()
    }
}

// ─── MailboxCache trait ───────────────────────────────────────────────────────

/// Minimum surface the compiler needs for mailbox lookups.
/// Phase 1 implementation: `EmptyMailboxCache` always returns `None`.
/// Phase 2 implementation: `nmp-nip65::InMemoryMailboxCache`.
pub trait MailboxCache: Send + Sync {
    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot>;
    /// Snapshot of all known entries for plan-id hashing.
    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)>;
    /// Monotonic generation counter — advances on every accepted `put`.
    fn generation(&self) -> u64;
    /// Request a background probe for a pubkey whose mailbox is unknown.
    ///
    /// Phase 1: no-op. Phase 2: the actor wires this to an `IndexerProbe`
    /// action that fetches the author's kind:10002 from the indexer set,
    /// then calls `put()` on cache arrival, triggering a recompile.
    ///
    /// Design: `docs/design/subscription-compilation/compiler.md` §3.2
    fn request_probe(&self, _pubkey: &Pubkey) {
        // Default: no-op. Implementations that own an action channel override this.
    }
}

// ─── EmptyMailboxCache ───────────────────────────────────────────────────────

/// Phase 1 stub: no mailbox data. All authors fall back to the indexer set.
pub struct EmptyMailboxCache;

impl MailboxCache for EmptyMailboxCache {
    fn get(&self, _pubkey: &Pubkey) -> Option<MailboxSnapshot> {
        None
    }
    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)> {
        Vec::new()
    }
    fn generation(&self) -> u64 {
        0
    }
}

// ─── InMemoryMailboxCache ────────────────────────────────────────────────────

/// Simple in-memory mailbox cache for tests and the planner harness.
#[derive(Default)]
pub struct InMemoryMailboxCache {
    data: HashMap<Pubkey, MailboxSnapshot>,
    generation: u64,
}

impl InMemoryMailboxCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&mut self, pubkey: Pubkey, snapshot: MailboxSnapshot) {
        self.data.insert(pubkey, snapshot);
        self.generation = self.generation.saturating_add(1);
    }
}

impl MailboxCache for InMemoryMailboxCache {
    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot> {
        self.data.get(pubkey).cloned()
    }
    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)> {
        self.data.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
    fn generation(&self) -> u64 {
        self.generation
    }
}
