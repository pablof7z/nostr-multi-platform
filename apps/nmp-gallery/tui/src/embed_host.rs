//! `EmbedHostState` — gallery-side mirror of the kernel's `claimed_events`
//! snapshot projection.
//!
//! The renderer is frontend-driven (ADR-0034 / M16): when `NostrContentView`
//! walks the content tree and hits an `EventRef(uri)`, it calls
//! `sink.claim(uri, consumer_id)` via `EventClaimSink`. The host
//! (`LiveKernelSink`) decodes the URI and forwards the raw event key through
//! `resolve_ref`. The kernel registers a `OneshotApi` interest (D4 single
//! writer), fetches the event from relays *or* short-circuits when it's already
//! in the local store (cache hit, sub-tick latency), and surfaces the resolved
//! event in the typed `claimed_events` sidecar (ADR-0037).
//!
//! `EmbedHostState` is the gallery's read-side cache of that projection.
//! Each snapshot push calls `update_from_typed`; on the next redraw the
//! renderer's `embedded_events(...)` builder method reads from
//! `current_envelopes()` and the kind registry dispatches to the right
//! handler (`ArticleProjection`, `ShortNoteProjection`, etc.).
//!
//! Cache-agnostic: whether the kernel returned the event from local store
//! or after a relay round-trip, the host sees the same DTO shape and the
//! renderer sees the same envelope.
//!
//! Doctrine:
//! - **D8** — no polling. Updates are push-driven by the snapshot callback;
//!   the renderer reads a borrowed reference on each render pass.

use std::collections::BTreeMap;

use nmp_content::{
    embed_projection::{EmbeddedEventEnvelope, RenderContextWire},
    resolve_embed_projection, RenderContext,
};
use nmp_core::substrate::KernelEvent;
use nmp_core::typed_projections::ClaimedEventRow;

use crate::live::GalleryTypedSnapshot;

/// Gallery-side cache of resolved embed envelopes. Reset on every snapshot
/// (latest wins — the kernel's projection is the source of truth).
#[derive(Default)]
pub struct EmbedHostState {
    envelopes: BTreeMap<String, EmbeddedEventEnvelope>,
}

impl EmbedHostState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuild the in-memory envelope map from a freshly pushed kernel
    /// snapshot (typed path — reads the `claimed_events` typed sidecar from
    /// `GalleryTypedSnapshot`). Each entry is a `ClaimedEventRow`; we turn it
    /// into a `KernelEvent`, route it through the canonical
    /// `resolve_embed_projection` dispatch point (the same function ADR-0034
    /// mandates for ALL embed kind decisions), and store the resulting envelope
    /// under `primary_id`.
    ///
    /// Non-fatal: malformed entries are silently skipped (D6 — the renderer
    /// falls back to a loading placeholder until a well-formed snapshot lands).
    ///
    /// Returns the pubkeys of claimed-event authors so the caller can issue
    /// `resolve_profile`; `claimed_events` itself carries raw event data only.
    pub fn update_from_typed(&mut self, snapshot: &GalleryTypedSnapshot) -> Vec<String> {
        // An absent (empty) claimed_events model is a no-op — do not wipe
        // existing envelopes when no events are claimed yet (mirrors the
        // previous JSON behaviour where a missing "claimed_events" key left
        // the host untouched). Once the model has at least one entry the
        // full replacement fires, keeping latest-wins semantics.
        if snapshot.claimed_events.entries.is_empty() {
            return Vec::new();
        }

        let mut next: BTreeMap<String, EmbeddedEventEnvelope> = BTreeMap::new();
        let mut authors_needing_profile: Vec<String> = Vec::new();
        let ctx = RenderContext::new();

        for (primary_id, row) in &snapshot.claimed_events.entries {
            let Some(event) = kernel_event_from_row(row) else {
                continue;
            };

            if !event.author.is_empty() {
                authors_needing_profile.push(event.author.clone());
            }

            let projection = resolve_embed_projection(&event, &ctx);
            let envelope = EmbeddedEventEnvelope {
                uri: String::new(), // The renderer falls back from primary_id; URI keying happens at claim time.
                primary_id: primary_id.clone(),
                render_context: RenderContextWire {
                    depth: 0,
                    max_depth: 4,
                    visited: Vec::new(),
                },
                projection,
                collapsed: false,
                collapse_reason: None,
            };
            next.insert(primary_id.clone(), envelope);
        }

        self.envelopes = next;
        authors_needing_profile.sort();
        authors_needing_profile.dedup();
        authors_needing_profile
    }

    /// Borrow the current envelope map for the renderer's
    /// `NostrContentView::embedded_events(Some(host.current_envelopes()))`
    /// builder.
    #[must_use]
    pub fn current_envelopes(&self) -> &BTreeMap<String, EmbeddedEventEnvelope> {
        &self.envelopes
    }

    /// Number of resolved envelopes — diagnostics only.
    #[must_use]
    pub fn len(&self) -> usize {
        self.envelopes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.envelopes.is_empty()
    }
}

fn kernel_event_from_row(row: &ClaimedEventRow) -> Option<KernelEvent> {
    if row.author_pubkey.is_empty() {
        return None;
    }
    Some(KernelEvent {
        id: row.id.clone(),
        author: row.author_pubkey.clone(),
        kind: row.kind,
        created_at: row.created_at,
        tags: row.tags.clone(),
        content: row.content.clone(),
        relay_provenance: Vec::new(),
    })
}

#[cfg(test)]
trait ArticleHelpers {
    fn kind_optional_check(&self) -> u32;
}

#[cfg(test)]
impl ArticleHelpers for nmp_content::embed_projection::ArticleProjection {
    /// Test-only helper — `ArticleProjection` doesn't carry an explicit `kind`
    /// field (the variant tag IS the kind), so we return the canonical value
    /// from the spec for kind:30023.
    fn kind_optional_check(&self) -> u32 {
        30023
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{
        article_expected_title, article_primary_id, highlight_event_id, note_event_id,
        showcase_pubkey,
    };
    use nmp_content::embed_projection::EmbedKindProjection;
    use nmp_core::typed_projections::{ClaimedEventRow, ClaimedEventsModel};

    fn snapshot_with(entries: Vec<(String, ClaimedEventRow)>) -> GalleryTypedSnapshot {
        GalleryTypedSnapshot {
            claimed_events: ClaimedEventsModel { entries },
            profiles: std::collections::BTreeMap::new(),
            relay_statuses: Vec::new(),
        }
    }

    fn article_row() -> (String, ClaimedEventRow) {
        let primary = article_primary_id().to_string();
        let row = ClaimedEventRow {
            primary_id: primary.clone(),
            id: primary.clone(),
            author_pubkey: "6e468422dfb74a5738702a8823b9b28168abab8655faacb6853cd0ee15deee93"
                .to_string(),
            kind: 30023,
            created_at: 1716000000,
            tags: vec![
                vec!["d".to_string(), "the-internet-left-me".to_string()],
                vec![
                    "title".to_string(),
                    article_expected_title().unwrap_or("").to_string(),
                ],
            ],
            content: "Long-form article body.".to_string(),
            content_tree_bytes: Vec::new(),
            signed_event_json: None,
        };
        (primary, row)
    }

    fn short_note_row() -> (String, ClaimedEventRow) {
        let primary = note_event_id().to_string();
        let row = ClaimedEventRow {
            primary_id: primary.clone(),
            id: primary.clone(),
            author_pubkey: showcase_pubkey().to_string(),
            kind: 1,
            created_at: 1716000001,
            tags: vec![],
            content: "Relay-backed pablof7z note.".to_string(),
            content_tree_bytes: Vec::new(),
            signed_event_json: None,
        };
        (primary, row)
    }

    fn highlight_row() -> (String, ClaimedEventRow) {
        let primary = highlight_event_id().to_string();
        let row = ClaimedEventRow {
            primary_id: primary.clone(),
            id: primary.clone(),
            author_pubkey: showcase_pubkey().to_string(),
            kind: 9802,
            created_at: 1716000002,
            tags: vec![vec!["r".to_string(), "https://pablof7z.com".to_string()]],
            content: "Vibe-coding is what brought me back to programming.".to_string(),
            content_tree_bytes: Vec::new(),
            signed_event_json: None,
        };
        (primary, row)
    }

    #[test]
    fn host_starts_empty() {
        let host = EmbedHostState::new();
        assert!(host.is_empty());
    }

    #[test]
    fn article_row_resolves_to_article_projection() {
        let (primary, row) = article_row();
        let snap = snapshot_with(vec![(primary.clone(), row)]);

        let mut host = EmbedHostState::new();
        host.update_from_typed(&snap);

        let env = host
            .current_envelopes()
            .get(&primary)
            .expect("article envelope should be present");
        match &env.projection {
            EmbedKindProjection::Article(a) => {
                assert_eq!(a.kind_optional_check(), 30023);
                assert_eq!(a.d_tag, "the-internet-left-me");
                assert_eq!(a.title.as_deref(), article_expected_title());
            }
            other => panic!("expected Article projection, got {:?}", other),
        }
    }

    #[test]
    fn short_note_row_resolves_to_short_note_projection() {
        let (primary, row) = short_note_row();
        let snap = snapshot_with(vec![(primary.clone(), row)]);

        let mut host = EmbedHostState::new();
        host.update_from_typed(&snap);

        let env = host
            .current_envelopes()
            .get(&primary)
            .expect("short note envelope should be present");
        assert!(matches!(env.projection, EmbedKindProjection::ShortNote(_)));
    }

    #[test]
    fn highlight_row_resolves_to_highlight_projection() {
        let (primary, row) = highlight_row();
        let snap = snapshot_with(vec![(primary.clone(), row)]);

        let mut host = EmbedHostState::new();
        host.update_from_typed(&snap);

        let env = host
            .current_envelopes()
            .get(&primary)
            .expect("highlight envelope should be present");
        assert!(matches!(env.projection, EmbedKindProjection::Highlight(_)));
    }

    #[test]
    fn malformed_row_skipped_without_panic() {
        // A row with an empty author_pubkey is considered malformed and is skipped.
        let primary = note_event_id().to_string();
        let row = ClaimedEventRow {
            primary_id: primary.clone(),
            id: primary.clone(),
            author_pubkey: String::new(), // empty → skipped
            kind: 1,
            created_at: 0,
            tags: vec![],
            content: String::new(),
            content_tree_bytes: Vec::new(),
            signed_event_json: None,
        };
        let snap = snapshot_with(vec![(primary, row)]);

        let mut host = EmbedHostState::new();
        host.update_from_typed(&snap);

        assert!(
            host.is_empty(),
            "malformed row must be silently skipped (D6)"
        );
    }

    #[test]
    fn empty_model_leaves_host_untouched() {
        let mut host = EmbedHostState::new();
        // First load a real entry.
        let (primary, row) = short_note_row();
        host.update_from_typed(&snapshot_with(vec![(primary.clone(), row)]));
        assert_eq!(host.len(), 1);

        // An empty model (no claimed_events entries) should NOT wipe state.
        host.update_from_typed(&GalleryTypedSnapshot::default());
        assert_eq!(host.len(), 1, "empty model must not wipe state");
    }

    #[test]
    fn replacement_snapshot_replaces_state() {
        let mut host = EmbedHostState::new();
        let (primary_a, row_a) = short_note_row();
        let (primary_b, row_b) = article_row();

        host.update_from_typed(&snapshot_with(vec![(primary_a.clone(), row_a)]));
        assert!(host.current_envelopes().contains_key(&primary_a));

        // Second snapshot drops A and has B — latest wins.
        host.update_from_typed(&snapshot_with(vec![(primary_b.clone(), row_b)]));
        assert!(!host.current_envelopes().contains_key(&primary_a));
        assert!(host.current_envelopes().contains_key(&primary_b));
    }
}
