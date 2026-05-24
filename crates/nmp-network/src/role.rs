//! Transport-lane discriminator. Step 8 phase A — moved verbatim from
//! `nmp-core::relay::RelayRole`. `nmp-core` re-exports the type under its
//! prior path (`nmp_core::RelayRole`) for backward compatibility.
//!
//! **Not a routing source (T105).** The actual wire target is the resolved
//! `OutboundMessage::relay_url`. `RelayRole` only buckets relay-health rows,
//! NIP-42 driver state, and `wire_subs` for the diagnostic surface.
//!
//! V-01 Stage 3 — promoted to `pub` so the wasm32 `BrowserRelayDriver` in
//! `nmp-wasm` can name the role when handing a frame to
//! `KernelReducer::handle_relay_frame`. Substrate-grade (D0): the type
//! carries no app/protocol nouns.

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RelayRole {
    Content,
    Indexer,
    /// NIP-47 Nostr Wallet Connect relay. Spawned on demand when a wallet is
    /// connected; NOT included in `all()` so it does not block the startup
    /// bootstrap gate or appear in the standard relay-statuses projection.
    //
    // Constructed only under nmp-core's `#[cfg(feature = "wallet")]`
    // (actor/commands/wallet.rs); always-callable here because nmp-network
    // does not know about Cargo features in downstream crates.
    Wallet,
}

impl RelayRole {
    /// Bootstrap-only roles (spawned at start, gate for startup REQs).
    /// `Wallet` is excluded: it spawns on demand, not at startup.
    #[must_use]
    pub fn all() -> [Self; 2] {
        [Self::Content, Self::Indexer]
    }

    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Indexer => "indexer",
            Self::Wallet => "wallet",
        }
    }
}
