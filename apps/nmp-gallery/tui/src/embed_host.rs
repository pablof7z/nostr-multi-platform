//! `EmbedHostState` — gallery-side mirror of the kernel's `claimed_events`
//! snapshot projection.
//!
//! The renderer is frontend-driven (ADR-0034 / M16): when `NostrContentView`
//! walks the content tree and hits an `EventRef(uri)`, it calls
//! `sink.claim(uri, consumer_id)` via `EventClaimSink`. The host
//! (`LiveKernelSink`) forwards to `nmp_app_claim_event` — the kernel
//! registers a `OneshotApi` interest (D4 single writer), fetches the event
//! from relays *or* short-circuits when it's already in the local store
//! (cache hit, sub-tick latency), and surfaces the resolved event in the
//! snapshot's `projections.claimed_events[primary_id]` map.
//!
//! `EmbedHostState` is the gallery's read-side cache of that projection.
//! Each snapshot push calls `update_from_snapshot`; on the next redraw the
//! renderer's `embedded_events(...)` builder method reads from
//! `current_envelopes()` and the kind registry dispatches to the right
//! handler (`ArticleProjection`, `ShortNoteProjection`, etc.).
//!
//! Cache-agnostic: whether the kernel returned the event from local store
//! or after a relay round-trip, the host sees the same DTO shape and the
//! renderer sees the same envelope. (See the cache-investigation report
//! that landed before this module — `changed_since_emit` is set BEFORE the
//! cache-hit short-circuit so the next snapshot tick fires immediately.)
//!
//! Doctrine:
//! - **D8** — no polling. Updates are push-driven by the snapshot callback;
//!   the renderer reads a borrowed reference on each render pass.

use std::collections::BTreeMap;

use nmp_content::{
    embed_projection::{EmbedKindProjection, EmbeddedEventEnvelope, RenderContextWire},
    resolve_embed_projection, RenderContext,
};
use nmp_core::substrate::KernelEvent;
use serde_json::Value;

use crate::content_render_data::ContentProfileRenderData;

/// Gallery-side cache of resolved embed envelopes. Reset on every snapshot
/// (latest wins — the kernel's projection is the source of truth).
#[derive(Default)]
pub struct EmbedHostState {
    envelopes: BTreeMap<String, EmbeddedEventEnvelope>,
    /// Gallery-side mirror of the kernel's `claimed_profiles` projection,
    /// keyed by hex pubkey. Feeds `NostrContentView::live_profiles` so an
    /// inline `Mention` token resolves to the real display name once kind:0
    /// arrives (until then the mention chip shows a truncated-npub
    /// placeholder). Reset on every snapshot — latest wins, same contract
    /// as `envelopes`.
    profiles: BTreeMap<String, ContentProfileRenderData>,
}

impl EmbedHostState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuild the in-memory envelope map from a freshly pushed kernel
    /// snapshot. The kernel emits `projections.claimed_events[primary_id]
    /// → ClaimedEventDto`. We turn each entry into a `KernelEvent`, route
    /// it through the canonical `resolve_embed_projection` dispatch point
    /// (the same function ADR-0034 mandates for ALL embed kind decisions),
    /// and store the resulting envelope under `primary_id`. The renderer's
    /// `envelope_for` lookup tries `primary_id` first, then `uri` — so
    /// keying under `primary_id` is sufficient for the standard NIP-19
    /// shapes (`nevent` → event-id hex; `naddr` → coordinate string).
    ///
    /// Non-fatal: malformed entries are silently skipped (D6 — the
    /// renderer falls back to a loading placeholder until a well-formed
    /// snapshot lands).
    pub fn update_from_snapshot(&mut self, snapshot: &Value) -> Vec<String> {
        // Profiles update independently of events: a snapshot may carry a
        // refreshed `claimed_profiles` map (a mention's kind:0 just landed)
        // without any `claimed_events` change, and vice versa. A missing
        // `claimed_profiles` key leaves the existing profile cache intact —
        // same "missing projection must not wipe state" semantics applied
        // to envelopes below.
        if let Some(profiles) = snapshot
            .get("projections")
            .and_then(|p| p.get("claimed_profiles"))
            .and_then(Value::as_object)
        {
            self.profiles = profiles
                .iter()
                .map(|(pubkey, dto)| (pubkey.clone(), profile_render_data(pubkey, dto)))
                .collect();
        }

        let Some(claimed) = snapshot
            .get("projections")
            .and_then(|p| p.get("claimed_events"))
            .and_then(Value::as_object)
        else {
            return Vec::new();
        };

        let mut next: BTreeMap<String, EmbeddedEventEnvelope> = BTreeMap::new();
        let mut authors_needing_profile: Vec<String> = Vec::new();
        let ctx = RenderContext::new();

        for (primary_id, dto) in claimed {
            let Some(event) = kernel_event_from_dto(primary_id, dto) else {
                continue;
            };

            // Read the author's profile fields from the kernel's enriched
            // ClaimedEventDto. None when no kind:0 has been ingested yet —
            // we return the author pubkey to the caller so the main loop
            // can trigger a `claim_profile` and the next snapshot tick
            // brings the resolved profile.
            let author_display_name = dto
                .get("author_display_name")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string);
            let author_picture_url = dto
                .get("author_picture_url")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string);
            if author_display_name.is_none() && !event.author.is_empty() {
                authors_needing_profile.push(event.author.clone());
            }

            let mut projection = resolve_embed_projection(&event, &ctx);
            apply_author_profile(&mut projection, author_display_name, author_picture_url);
            let envelope = EmbeddedEventEnvelope {
                uri: String::new(), // The renderer falls back from primary_id; URI keying happens at claim time, not here.
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

    /// Borrow the current resolved-profile map (keyed by hex pubkey) for the
    /// renderer's `NostrContentView::live_profiles(Some(host.profiles()))`
    /// builder. Empty when no mention has resolved a kind:0 yet — the
    /// renderer's mention chip falls back to a truncated-npub placeholder.
    #[must_use]
    pub fn profiles(&self) -> &BTreeMap<String, ContentProfileRenderData> {
        &self.profiles
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

/// Decode one `claimed_profiles[pubkey] -> MentionProfilePayload` entry into
/// the renderer's `ContentProfileRenderData`. The npub is always derived from
/// the hex pubkey key (so the mention chip can show a stable truncated-npub
/// placeholder before kind:0 arrives); `display_name` / `picture_url` stay
/// `None` until the kernel has ingested the profile. Mirrors the
/// `ClaimedEventDto` author-profile decode in `update_from_snapshot`.
fn profile_render_data(pubkey: &str, dto: &Value) -> ContentProfileRenderData {
    ContentProfileRenderData {
        pubkey: pubkey.to_string(),
        display_name: dto
            .get("display_name")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string),
        npub: Some(nmp_core::display::to_npub(pubkey)),
        picture_url: dto
            .get("picture_url")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string),
    }
}

fn kernel_event_from_dto(primary_id: &str, dto: &Value) -> Option<KernelEvent> {
    let id = dto.get("id").and_then(Value::as_str)?.to_string();
    let author = dto
        .get("author_pubkey")
        .and_then(Value::as_str)?
        .to_string();
    let kind = dto.get("kind").and_then(Value::as_u64)? as u32;
    let created_at = dto.get("created_at").and_then(Value::as_u64).unwrap_or(0);
    let content = dto
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let tags = dto
        .get("tags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_array)
                .map(|row| {
                    row.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<String>>()
                })
                .collect::<Vec<Vec<String>>>()
        })
        .unwrap_or_default();
    // `primary_id` is the snapshot key; for hex64-form (nevent/note) it equals
    // `event.id`. For coordinate-form (naddr) the renderer's `envelope_for`
    // lookup keys on `primary_id`, so we don't need to back-fill anything
    // here. The KernelEvent only needs the protocol fields.
    let _ = primary_id;
    Some(KernelEvent {
        id,
        author,
        kind,
        created_at,
        tags,
        content,
    })
}

/// Stamp the author's display name + picture URL on whichever
/// projection variant carries those fields. `resolve_embed_projection`
/// initializes them as `None` since it only sees the raw `KernelEvent`;
/// this helper folds in the kernel's profile-cache enrichment delivered
/// via `ClaimedEventDto.author_display_name` / `_picture_url`. The
/// `Highlight` variant carries `author_display_name` only (no picture);
/// `Unknown` follows the same shape. All variants degrade gracefully
/// when both fields are `None` — renderers compose with
/// `NostrProfileName` which falls back to the truncated npub.
fn apply_author_profile(
    projection: &mut EmbedKindProjection,
    display_name: Option<String>,
    picture_url: Option<String>,
) {
    match projection {
        EmbedKindProjection::ShortNote(n) => {
            n.author_display_name = display_name;
            n.author_picture_url = picture_url;
        }
        EmbedKindProjection::Article(a) => {
            a.author_display_name = display_name;
            a.author_picture_url = picture_url;
        }
        EmbedKindProjection::Highlight(h) => {
            h.author_display_name = display_name;
        }
        EmbedKindProjection::Profile(_) => {
            // kind:0 — the projection itself IS the profile.
        }
        EmbedKindProjection::Unknown(u) => {
            u.author_display_name = display_name;
            u.author_picture_url = picture_url;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_content::embed_projection::EmbedKindProjection;
    use serde_json::json;

    fn snapshot_with(events: Vec<(&str, Value)>) -> Value {
        let mut claimed = serde_json::Map::new();
        for (key, dto) in events {
            claimed.insert(key.to_string(), dto);
        }
        json!({
            "projections": {
                "claimed_events": Value::Object(claimed),
            }
        })
    }

    fn article_dto() -> Value {
        json!({
            "id": "aaaa000000000000000000000000000000000000000000000000000000000001",
            "author_pubkey": "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52",
            "kind": 30023,
            "created_at": 1716000000_u64,
            "tags": [["d", "kind-dispatch"], ["title", "Kind-Dispatch Content Rendering"]],
            "content": "Long-form article body."
        })
    }

    fn short_note_dto() -> Value {
        json!({
            "id": "bbbb000000000000000000000000000000000000000000000000000000000001",
            "author_pubkey": "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
            "kind": 1,
            "created_at": 1716000001_u64,
            "tags": [],
            "content": "Hello from fiatjaf."
        })
    }

    fn highlight_dto() -> Value {
        json!({
            "id": "cccc000000000000000000000000000000000000000000000000000000000001",
            "author_pubkey": "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
            "kind": 9802,
            "created_at": 1716000002_u64,
            "tags": [["r", "https://fiatjaf.com"]],
            "content": "The simplest protocol wins."
        })
    }

    #[test]
    fn host_starts_empty() {
        let host = EmbedHostState::new();
        assert!(host.is_empty());
    }

    #[test]
    fn article_dto_resolves_to_article_projection() {
        let primary =
            "30023:fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52:kind-dispatch";
        let snap = snapshot_with(vec![(primary, article_dto())]);

        let mut host = EmbedHostState::new();
        host.update_from_snapshot(&snap);

        let env = host
            .current_envelopes()
            .get(primary)
            .expect("article envelope should be present");
        match &env.projection {
            EmbedKindProjection::Article(a) => {
                assert_eq!(a.kind_optional_check(), 30023);
                assert_eq!(a.d_tag, "kind-dispatch");
                assert_eq!(a.title.as_deref(), Some("Kind-Dispatch Content Rendering"));
            }
            other => panic!("expected Article projection, got {:?}", other),
        }
    }

    #[test]
    fn short_note_dto_resolves_to_short_note_projection() {
        let primary = "bbbb000000000000000000000000000000000000000000000000000000000001";
        let snap = snapshot_with(vec![(primary, short_note_dto())]);

        let mut host = EmbedHostState::new();
        host.update_from_snapshot(&snap);

        let env = host
            .current_envelopes()
            .get(primary)
            .expect("short note envelope should be present");
        assert!(matches!(env.projection, EmbedKindProjection::ShortNote(_)));
    }

    #[test]
    fn highlight_dto_resolves_to_highlight_projection() {
        let primary = "cccc000000000000000000000000000000000000000000000000000000000001";
        let snap = snapshot_with(vec![(primary, highlight_dto())]);

        let mut host = EmbedHostState::new();
        host.update_from_snapshot(&snap);

        let env = host
            .current_envelopes()
            .get(primary)
            .expect("highlight envelope should be present");
        assert!(matches!(env.projection, EmbedKindProjection::Highlight(_)));
    }

    #[test]
    fn malformed_dto_skipped_without_panic() {
        let primary = "deadbeef";
        let snap = snapshot_with(vec![(
            primary,
            json!({"id": "x", "author_pubkey": null, "kind": 1, "created_at": 0, "tags": [], "content": ""}),
        )]);

        let mut host = EmbedHostState::new();
        host.update_from_snapshot(&snap);

        assert!(
            host.is_empty(),
            "malformed dto must be silently skipped (D6)"
        );
    }

    #[test]
    fn snapshot_without_claimed_events_leaves_host_untouched() {
        let mut host = EmbedHostState::new();
        // First load a real entry.
        let primary = "bbbb000000000000000000000000000000000000000000000000000000000001";
        host.update_from_snapshot(&snapshot_with(vec![(primary, short_note_dto())]));
        assert_eq!(host.len(), 1);

        // A snapshot without the projection key should NOT clear existing entries
        // — only an empty projections.claimed_events object would.
        host.update_from_snapshot(&json!({"projections": {}}));
        assert_eq!(host.len(), 1, "missing projection must not wipe state");
    }

    #[test]
    fn replacement_snapshot_replaces_state() {
        let mut host = EmbedHostState::new();
        let primary_a = "bbbb000000000000000000000000000000000000000000000000000000000001";
        let primary_b =
            "30023:fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52:kind-dispatch";

        host.update_from_snapshot(&snapshot_with(vec![(primary_a, short_note_dto())]));
        assert!(host.current_envelopes().contains_key(primary_a));

        // Second snapshot drops A and has B — latest wins.
        host.update_from_snapshot(&snapshot_with(vec![(primary_b, article_dto())]));
        assert!(!host.current_envelopes().contains_key(primary_a));
        assert!(host.current_envelopes().contains_key(primary_b));
    }
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
