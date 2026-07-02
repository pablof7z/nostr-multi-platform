//! `refs.event` claimed-event row payload.
//!
//! Owns [`ClaimedEventDto`], the per-event payload bundled into one
//! `refs.event` row. Surfaces the raw protocol fields a renderer needs to
//! resolve an embed without re-walking the store on the FFI side. Encoded into
//! the KCEV row payload from `kernel/ref_row_source.rs`.

use super::read_cache::StoredEvent;
use super::Serialize;

/// Per-event payload bundled into one `refs.event` row. Surfaces the raw
/// protocol fields a renderer needs to resolve an embed without re-walking the
/// store on the FFI side.
///
/// Keyed by `primary_id` in the outer NRRD row:
/// - hex-64 event id for nevent/note URIs (matches `StoredEvent.id`),
/// - `kind:pubkey:d_tag` coordinate string for naddr URIs (matches the
///   renderer-side `WireUri.primary_id`).
///
/// D0 — the name is intentionally generic ("event", not "embed"); the
/// kernel primitive that drives this row is event `resolve_ref` and
/// can carry any kind, not just embed-class events.
///
/// `pub(crate)` struct with `pub(super)` fields, encoded into the KCEV row
/// payload from `kernel/ref_row_source.rs`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct ClaimedEventDto {
    /// The projection key — either a hex-64 event id (nevent/note) or
    /// `kind:pubkey:d_tag` (naddr). Carried in the body too so a shell
    /// consuming a flat array of payloads keeps provenance.
    pub(super) primary_id: String,
    /// Canonical 64-hex event id of the resolved event (always the
    /// concrete event id, even when the URI was an addressable
    /// coordinate).
    pub(super) id: String,
    /// Author pubkey, hex (64 chars). Presentation layer formats for
    /// display.
    pub(super) author_pubkey: String,
    /// Event kind.
    pub(super) kind: u32,
    /// Unix-seconds `created_at`. Presentation layer formats relative
    /// time.
    pub(super) created_at: u64,
    /// Raw event tags. Renderers walk these for embed-specific fields
    /// (NIP-23 title, summary, image).
    pub(super) tags: Vec<Vec<String>>,
    /// Raw event content. NIP-23 article body, kind:1 note text, etc.
    pub(super) content: String,
    /// Parsed NFCT bytes for the typed KCEV row payload.
    #[serde(skip)]
    pub(super) content_tree_bytes: Vec<u8>,
    /// Canonical signed NIP-01 event JSON, including `sig`.
    ///
    /// Populated only for the generic `refs.event` Raw shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) signed_event_json: Option<String>,
}

impl ClaimedEventDto {
    /// Build a `ClaimedEventDto` from a kernel-side `StoredEvent`,
    /// stamping the caller-provided `primary_id` (which may be either
    /// the event id verbatim or an addressable coordinate string).
    /// Author profile data is resolved through `refs.profile` claims instead
    /// of being duplicated into claimed-event rows.
    pub(super) fn from_stored(primary_id: String, e: &StoredEvent) -> Self {
        Self {
            primary_id,
            id: e.id.clone(),
            author_pubkey: e.author.clone(),
            kind: e.kind,
            created_at: e.created_at,
            tags: e.tags.clone(),
            content: e.content.clone(),
            content_tree_bytes: Vec::new(),
            signed_event_json: None,
        }
    }

    pub(super) fn with_content_tree(mut self, content_tree_bytes: Vec<u8>) -> Self {
        self.content_tree_bytes = content_tree_bytes;
        self
    }

    pub(super) fn with_signed_event_json(mut self, signed_event_json: Option<String>) -> Self {
        self.signed_event_json = signed_event_json;
        self
    }
}
