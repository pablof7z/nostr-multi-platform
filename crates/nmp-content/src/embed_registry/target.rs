//! Embed-target identity types — the keys the registry refcounts and the
//! opaque [`ClaimHandle`] tokens it hands back.

use nmp_core::nip21::NostrUri;
use nmp_core::substrate::{EventId, KernelEvent};
use serde::{Deserialize, Serialize};

/// Stable identity for an embed target — covers both event-id-addressed
/// (`note1`/`nevent1`) and coordinate-addressed (`naddr1`) embeds.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum EmbedTarget {
    /// `nevent1…` / `note1…` — 32-byte hex event id.
    Event(EventId),
    /// `naddr1…` — `(kind, pubkey, d_tag)` coordinate.
    Address {
        /// Event kind.
        kind: u32,
        /// Author pubkey (hex).
        pubkey: String,
        /// `d` tag identifier.
        identifier: String,
    },
}

impl EmbedTarget {
    /// Project a [`NostrUri`] onto the embed-target shape. `Profile` URIs
    /// return `None` — they aren't embeds.
    #[must_use]
    pub fn from_uri(uri: &NostrUri) -> Option<Self> {
        match uri {
            NostrUri::Profile { .. } => None,
            NostrUri::Event { event_id, .. } => Some(Self::Event(event_id.clone())),
            NostrUri::Address {
                identifier,
                pubkey,
                kind,
                ..
            } => Some(Self::Address {
                kind: *kind,
                pubkey: pubkey.clone(),
                identifier: identifier.clone(),
            }),
        }
    }
}

/// Opaque handle returned by [`EmbedClaimRegistry::claim`]. Hold while the
/// embed is visible; pass to `release` when it scrolls offscreen.
///
/// [`EmbedClaimRegistry::claim`]: super::EmbedClaimRegistry::claim
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ClaimHandle {
    pub(super) target: EmbedTarget,
    pub(super) handle_id: u64,
}

impl ClaimHandle {
    /// The target this handle refcounts.
    #[must_use]
    pub fn target(&self) -> &EmbedTarget {
        &self.target
    }

    /// Per-handle unique id — distinguishes 2 distinct claims for the
    /// same target.
    #[must_use]
    pub fn handle_id(&self) -> u64 {
        self.handle_id
    }
}

/// Snapshot of a resolved embed event. Layer A doesn't need the full
/// `StoredEvent` shape — apps that want it look up the kernel store
/// directly using `id`. This is the minimum projection apps need to render
/// the embed card without a follow-up fetch.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ResolvedEvent {
    /// Hex event id.
    pub id: EventId,
    /// Hex author pubkey.
    pub author: String,
    /// Event kind.
    pub kind: u32,
    /// Unix seconds.
    pub created_at: u64,
    /// Raw content string (renderer tokenizes).
    pub content: String,
    /// Tag rows.
    pub tags: Vec<Vec<String>>,
}

impl From<&KernelEvent> for ResolvedEvent {
    fn from(e: &KernelEvent) -> Self {
        Self {
            id: e.id.clone(),
            author: e.author.clone(),
            kind: e.kind,
            created_at: e.created_at,
            content: e.content.clone(),
            tags: e.tags.clone(),
        }
    }
}
