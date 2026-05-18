//! Relay routing primitives.
//!
//! # T105 — outbox is the routing authority
//!
//! Wire relay selection is driven by the planner-resolved per-author write/read
//! relays (NIP-65), NOT by a hardcoded constant. Every [`OutboundMessage`]
//! carries an explicit `relay_url` — the resolved target the transport dials.
//! [`RelayRole`] survives ONLY as a transport-lane + diagnostics discriminator
//! (relay-health rows, NIP-42 driver buckets, `wire_subs` grouping); it is no
//! longer a routing source.
//!
//! [`BOOTSTRAP_DISCOVERY_RELAYS`] is the explicit, documented cold-start seed
//! used for the *very first* kind:10002 (NIP-65 relay-list) discovery fetch
//! when nothing is cached. It is NEVER the routing default — once an author's
//! kind:10002 is cached, the resolver routes to their declared relays and the
//! bootstrap seed is no longer consulted for that author (D3: outbox routing
//! automatic — `docs/product-spec/overview-and-dx.md` §1.5).

/// Cold-start discovery seed — the relays used for the very first NIP-65
/// (kind:10002) relay-list fetch when the store has no cached relay list for
/// the author yet. Gated to the cold-start discovery interest only; it is
/// never a routing fallback for content REQs once a relay list is known.
///
/// Two large public relays; neither is authoritative, both are best-effort.
/// Mirrors `crate::publish::DEFAULT_INDEXER_FALLBACK` (the publish-side seed)
/// so cold-start reads and the first publish converge on the same bootstrap.
pub(crate) const BOOTSTRAP_DISCOVERY_RELAYS: &[&str] =
    &["wss://relay.damus.io", "wss://purplepag.es"];

/// Diagnostics-only lane labels (relay-health rows, status payload). NOT a
/// routing source (T105) — they alias the cold-start bootstrap seeds purely so
/// the diagnostic surface has a stable string per lane until per-resolved-relay
/// health tracking lands (M11). Never consult these for wire routing.
pub(crate) const CONTENT_RELAY_URL: &str = BOOTSTRAP_DISCOVERY_RELAYS[0];
pub(crate) const INDEXER_RELAY_URL: &str = BOOTSTRAP_DISCOVERY_RELAYS[1];

pub(crate) const DEFAULT_VISIBLE_LIMIT: usize = 80;
pub(crate) const DEFAULT_EMIT_HZ: u32 = 4;
pub(crate) const TIMELINE_AUTHOR_LIMIT: usize = 500;
pub(crate) const TEST_NPUB: &str =
    "npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft";
pub(crate) const TEST_PUBKEY: &str =
    "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";
pub(crate) const FIATJAF_PUBKEY: &str =
    "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
pub(crate) const JB55_PUBKEY: &str =
    "32e1827635450ebb3c5a7d12c1f8e7b2b514439ac10a67eef3d9fd9c5c68e245";

/// Transport-lane + diagnostics discriminator.
///
/// **Not a routing source (T105).** The actual wire target is the resolved
/// `OutboundMessage::relay_url`. `RelayRole` only buckets relay-health rows,
/// NIP-42 driver state, and `wire_subs` for the diagnostic surface. The first
/// connection of each lane bootstraps on [`BOOTSTRAP_DISCOVERY_RELAYS`] purely
/// so the cold-start kind:10002 discovery fetch has a socket to leave on.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RelayRole {
    Content,
    Indexer,
    /// NIP-47 Nostr Wallet Connect relay. Spawned on demand when a wallet is
    /// connected; NOT included in `all()` so it does not block the startup
    /// bootstrap gate or appear in the standard relay-statuses projection.
    Wallet,
}

impl RelayRole {
    /// Bootstrap-only roles (spawned at start, gate for startup REQs).
    /// `Wallet` is excluded: it spawns on demand, not at startup.
    pub(crate) fn all() -> [Self; 2] {
        [Self::Content, Self::Indexer]
    }

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Indexer => "indexer",
            Self::Wallet => "wallet",
        }
    }

    /// Cold-start bootstrap URL for this lane's *first* socket. NOT a routing
    /// default — content/profile/thread REQs and publishes target the
    /// resolved `OutboundMessage::relay_url`, not this. Used only so the very
    /// first kind:10002 discovery fetch has a relay to dial before any NIP-65
    /// list is cached.
    pub(crate) fn bootstrap_url(self) -> &'static str {
        match self {
            // Distinct seeds per lane so the cold-start discovery fetch and
            // the seed-timeline bootstrap do not collide on one socket.
            Self::Content => BOOTSTRAP_DISCOVERY_RELAYS[0],
            Self::Indexer => BOOTSTRAP_DISCOVERY_RELAYS[1],
            // Wallet relay URL is dynamic (from NWC URI); no static bootstrap.
            Self::Wallet => "",
        }
    }

    /// Lane bootstrap URL, retained under the legacy name for the
    /// diagnostics/provenance call sites (relay-health rows, NIP-42 challenge
    /// host, store provenance when a frame's resolved URL is not threaded
    /// through). NOT a routing source — content/profile/thread/publish target
    /// the resolved `OutboundMessage::relay_url`. Alias of [`Self::bootstrap_url`].
    pub(crate) fn url(self) -> &'static str {
        self.bootstrap_url()
    }
}

/// One outbound wire frame addressed to a concrete, resolved relay.
///
/// `relay_url` is the routing authority (T105): the planner-resolved per-author
/// write relay (content/profile/thread), the active-account read relay
/// (hashtag firehose), the NIP-65 outbox fan-out target (publish), or the
/// cold-start [`BOOTSTRAP_DISCOVERY_RELAYS`] seed (first kind:10002 discovery).
/// `role` is retained only for the diagnostics/transport lane it belongs to.
#[derive(Clone, Debug)]
pub(crate) struct OutboundMessage {
    pub(crate) role: RelayRole,
    /// Resolved wire target. The transport dials this URL.
    pub(crate) relay_url: String,
    pub(crate) text: String,
}

impl OutboundMessage {
    /// Construct an outbound frame for a resolved relay URL on the given lane.
    #[allow(dead_code)] // T105 transition shim — used as fan-out matures.
    pub(crate) fn to_relay(role: RelayRole, relay_url: impl Into<String>, text: String) -> Self {
        Self {
            role,
            relay_url: relay_url.into(),
            text,
        }
    }
}
