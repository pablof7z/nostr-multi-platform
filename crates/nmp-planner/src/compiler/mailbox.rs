//! `MailboxCache` trait, `MailboxSnapshot`, and the planner-side implementations.
//!
//! This is the **planner-side** (Layer-2) mailbox seam the subscription
//! compiler consults. In production the kernel injects an adapter
//! (`nmp_core::kernel::mailboxes::KernelMailboxes`) that bridges the substrate
//! NIP-65 cache + the `DmInboxRelayLookup` (NIP-17) seam onto this trait; tests
//! and the planner harness use [`EmptyMailboxCache`] / [`InMemoryMailboxCache`].
//! It is intentionally NOT the same trait as the substrate-side
//! `nmp_core::substrate::MailboxCache` and the two cannot be collapsed — see the
//! durable-bridge rationale on the [`MailboxCache`] trait below (#967).
//!
//! Design: `docs/design/subscription-compilation/compiler.md` §3.1
//! Doctrine: D3 (outbox routing automatic).

use crate::interest::{Pubkey, RelayUrl};
use std::collections::HashMap;

// ─── MailboxSnapshot ─────────────────────────────────────────────────────────

/// Minimal mailbox snapshot used by the compiler — the read/write/both relay
/// sets for one author.
///
/// `write_relays` + `both_relays` drive the Outbox direction; `read_relays` +
/// `both_relays` drive the Inbox direction (`#p` interests — DMs, notifications).
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
    #[must_use]
    pub fn has_inbox_relays(&self) -> bool {
        !self.read_relays.is_empty() || !self.both_relays.is_empty()
    }
}

// ─── MailboxCache trait ───────────────────────────────────────────────────────

/// Minimum surface the compiler needs for mailbox lookups.
///
/// This is the **planner-side** (Layer-2) trait. It is intentionally distinct
/// from the substrate-side `nmp_core::substrate::MailboxCache` (Layer 3, NIP-65
/// only): the two live on opposite sides of a hard layer boundary and are
/// bridged by `nmp-core`'s `KernelMailboxes` adapter (#967). `dm_inbox_relays`
/// here is a thin facade over the substrate `DmInboxRelayLookup` seam, not a
/// second data store. In production the kernel injects the adapter; tests and
/// the planner harness use [`EmptyMailboxCache`] / [`InMemoryMailboxCache`].
pub trait MailboxCache: Send + Sync {
    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot>;
    /// NIP-17 kind:10050 DM inbox relays for `pubkey`.
    ///
    /// This is deliberately separate from [`Self::get`]. Generic `#p` inbox
    /// routing consumes kind:10002 read relays; NIP-17 gift-wrap inbox routing
    /// consumes kind:10050 relays and must fail closed when this returns `None`.
    fn dm_inbox_relays(&self, _pubkey: &Pubkey) -> Option<Vec<RelayUrl>> {
        None
    }
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
    dm_data: HashMap<Pubkey, Vec<RelayUrl>>,
    generation: u64,
}

impl InMemoryMailboxCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&mut self, pubkey: Pubkey, snapshot: MailboxSnapshot) {
        self.data.insert(pubkey, snapshot);
        self.generation = self.generation.saturating_add(1);
    }

    pub fn put_dm_relays(&mut self, pubkey: Pubkey, relays: Vec<RelayUrl>) {
        if relays.is_empty() {
            self.dm_data.remove(&pubkey);
        } else {
            self.dm_data.insert(pubkey, relays);
        }
        self.generation = self.generation.saturating_add(1);
    }
}

impl MailboxCache for InMemoryMailboxCache {
    fn get(&self, pubkey: &Pubkey) -> Option<MailboxSnapshot> {
        self.data.get(pubkey).cloned()
    }
    fn dm_inbox_relays(&self, pubkey: &Pubkey) -> Option<Vec<RelayUrl>> {
        self.dm_data
            .get(pubkey)
            .filter(|relays| !relays.is_empty())
            .cloned()
    }
    fn snapshot_all(&self) -> Vec<(Pubkey, MailboxSnapshot)> {
        self.data
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
    fn generation(&self) -> u64 {
        self.generation
    }
}
