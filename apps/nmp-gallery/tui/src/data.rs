//! Initial gallery references — what every page needs at frame zero.
//!
//! Live-only (ADR-0034 / M16): there is no fixture mode, no hardcoded
//! embed envelopes, no `load_images: bool` knob that toggles static payloads. The
//! gallery boots the kernel and carries real Nostr references; profile and
//! event fields arrive through relay-backed kernel snapshot projections.
//!
//! Embedded events do NOT live here. The renderer is frontend-driven:
//! when `NostrContentView` hits an `EventRef(uri)` it calls
//! `sink.claim(uri, …)`, the kernel fetches (cache or relay), and the
//! resolved envelopes flow through `EmbedHostState` (see `embed_host.rs`).
//! Renderers consume the host's envelope map at render time, not a
//! static field on `ContentExample`.

use std::collections::HashMap;

use nmp_app_gallery::showcase;
use nmp_content::{tokenize_with_kind, RenderMode};
use nmp_core::display::{short_npub, to_npub};
use ratatui_image::protocol::Protocol;
use serde_json::Value;

use crate::{
    content_render_data::ContentRenderData, content_tree_wire::ContentTreeWire,
    profile_wire::ProfileWire,
};

/// Real Nostr references shared by every NmpGallery host.
///
/// The source of truth is `apps/nmp-gallery/showcase-references.json`,
/// embedded by `nmp-app-gallery`. TUI does not duplicate pubkeys, event ids,
/// naddrs, nevents, or relay roles.
pub fn showcase_pubkey() -> &'static str {
    showcase::pubkey_hex()
}

pub fn showcase_npub() -> &'static str {
    showcase::npub()
}

pub fn article_naddr() -> &'static str {
    showcase::article_uri()
}

pub fn article_primary_id() -> &'static str {
    showcase::article_primary_id()
}

#[cfg(test)]
pub fn article_expected_title() -> Option<&'static str> {
    showcase::references().article.expected_title.as_deref()
}

pub fn note_nevent() -> &'static str {
    showcase::note_uri()
}

pub fn note_event_id() -> &'static str {
    showcase::note_primary_id()
}

pub fn highlight_nevent() -> &'static str {
    showcase::highlight_uri()
}

pub fn highlight_event_id() -> &'static str {
    showcase::highlight_primary_id()
}

pub struct GalleryData {
    /// Hex pubkey of the showcase's primary author. The user-* components
    /// resolve their `ProfileWire` reactively from `LiveProfileMap` at
    /// render time — `GalleryData` carries the *identity* (a pubkey), never
    /// a snapshot of profile fields. Kind:0 metadata flows in through the
    /// kernel snapshot, not through this struct's initialization.
    pub primary_pubkey: String,
    pub avatar_image: Option<Protocol>,
    pub avatar_image_compact: Option<Protocol>,
    pub media_images: Vec<MediaProtocol>,
    pub content_core: ContentExample,
    pub content_minimal: ContentExample,
    pub content_view: ContentExample,
    pub content_mention_chip: ContentExample,
    pub content_media_grid: ContentExample,
    pub content_quote_card: ContentExample,

    pub embed_article: ContentExample,
    pub embed_profile: ContentExample,
    pub embed_note: ContentExample,
    pub embed_highlight: ContentExample,
}

pub struct ContentExample {
    pub scenario_id: String,
    pub title: String,
    pub tree: ContentTreeWire,
    pub render_data: ContentRenderData,
}

pub struct MediaProtocol {
    pub url: String,
    pub protocol: Protocol,
}

/// Reactive store of resolved `ProfileWire`s keyed by hex pubkey.
///
/// This is the "every app gets this for free" layer: instead of each app
/// hand-extracting kind:0 fields from the kernel snapshot and stuffing them
/// into bespoke state, the app holds one `LiveProfileMap`, calls
/// `update_from_snapshot` on every snapshot tick, and `resolve(pubkey)` at
/// render time. The map fills itself from the kernel's `mention_profiles`
/// and `author_view.profile` projections — there is no app-side
/// field-by-field copying and no invented profile label.
#[derive(Default)]
pub struct LiveProfileMap {
    profiles: HashMap<String, ProfileWire>,
}

impl LiveProfileMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ingest a kernel snapshot, updating the resolved-profile map.
    ///
    /// Precedence (highest to lowest):
    /// 1. `claimed_profiles` — component-owned full ProfileCard
    /// 2. `author_view.profile` — full ProfileCard when has_profile=true
    /// 3. `mention_profiles` — only-if-absent (lightweight: display_name + picture_url)
    ///
    /// Three projections feed this:
    /// - `projections.claimed_profiles` — `{ pubkey: ProfileCard }`, emitted
    ///   for component-owned profile claims. This is the user-avatar happy path.
    /// - `projections.author_view.profile` — the full `ProfileCard`
    ///   (`pubkey, npub, display_name, picture_url, nip05, about,
    ///   has_profile`). Richer than `mention_profiles`, so when present and
    ///   `has_profile == true` it *overrides* any same-pubkey entry from
    ///   `mention_profiles`.
    /// - `projections.mention_profiles` — `{ pubkey: { pubkey, display_name,
    ///   picture_url } }`. The lightweight per-mention payload (no nip05 /
    ///   about). Only fills gaps: if a pubkey is already in the map from
    ///   claimed_profiles or author_view.profile, mention_profiles is skipped.
    ///   Establishes a profile entry for every author the kernel has kind:0 for
    ///   among the visible items.
    pub fn update_from_snapshot(&mut self, snapshot: &Value) {
        let Some(projections) = snapshot.get("projections") else {
            return;
        };

        // Step 1: Apply claimed_profiles (highest priority)
        if let Some(claimed_profiles) = projections
            .get("claimed_profiles")
            .and_then(Value::as_object)
        {
            for (pubkey, profile) in claimed_profiles {
                self.apply_profile_card(pubkey, profile);
            }
        }

        // Step 2: Apply author_view.profile (second priority, overwrites mention_profiles only)
        if let Some(profile) = projections
            .get("author_view")
            .and_then(|av| av.get("profile"))
        {
            self.apply_profile_card("", profile);
        }

        // Step 3: Apply mention_profiles only-if-absent (lowest priority)
        if let Some(mention_profiles) = projections
            .get("mention_profiles")
            .and_then(Value::as_object)
        {
            for (pubkey, payload) in mention_profiles {
                // Only fill gaps: skip if pubkey already in map
                if !self.profiles.contains_key(pubkey) {
                    let display_name = string_field(payload, "display_name");
                    let picture_url = string_field(payload, "picture_url");
                    let wire = self.entry_for(pubkey);
                    wire.display_name = display_name;
                    wire.picture_url = picture_url;
                }
            }
        }
    }

    /// Resolve the `ProfileWire` for `pubkey`, falling back to a name-less
    /// wire (pubkey + npub + npub_short, everything else `None`) when no
    /// kind:0 has arrived yet. The fallback carries NO invented display name —
    /// the presentation layer renders the truncated npub until the kernel
    /// delivers real metadata.
    pub fn resolve(&self, pubkey: &str) -> ProfileWire {
        self.profiles
            .get(pubkey)
            .cloned()
            .unwrap_or_else(|| profile_wire_for_pubkey(pubkey))
    }

    /// Get-or-insert the wire for `pubkey`, seeding the identity-only fields
    /// (`pubkey`, `npub`, `npub_short`) so callers only touch metadata.
    fn entry_for(&mut self, pubkey: &str) -> &mut ProfileWire {
        self.profiles
            .entry(pubkey.to_string())
            .or_insert_with(|| profile_wire_for_pubkey(pubkey))
    }

    fn apply_profile_card(&mut self, fallback_pubkey: &str, profile: &Value) {
        let is_real = profile
            .get("has_profile")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !is_real {
            return;
        }
        let pubkey = string_field(profile, "pubkey").unwrap_or_else(|| fallback_pubkey.to_string());
        if pubkey.is_empty() {
            return;
        }
        let display_name = string_field(profile, "display_name");
        let picture_url = string_field(profile, "picture_url");
        let nip05 = string_field(profile, "nip05");
        let about = string_field(profile, "about");
        let wire = self.entry_for(&pubkey);
        wire.display_name = display_name;
        wire.picture_url = picture_url;
        wire.nip05 = nip05;
        wire.about = about;
    }
}

/// Build a name-less `ProfileWire` for `pubkey`: identity fields only
/// (`pubkey`, `npub`, `npub_short`), every kind:0-derived field `None`. The
/// `ProfileWire::display()` fallback renders `npub_short`, so this is the
/// honest "no profile yet" state — never a fabricated name.
pub fn profile_wire_for_pubkey(pubkey: &str) -> ProfileWire {
    ProfileWire {
        pubkey: pubkey.to_string(),
        display_name: None,
        about: None,
        picture_url: None,
        nip05: None,
        npub: to_npub(pubkey),
        npub_short: short_npub(pubkey),
    }
}

/// Read a string field from a JSON object, treating empty strings and
/// missing/null as `None`. The kernel emits `nip05`/`about` as plain
/// (possibly empty) strings and `display_name`/`picture_url` as nullable —
/// this normalises both to `Option<String>`.
fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

impl GalleryData {
    /// Initial gallery state for the live program. `primary_pubkey` is the
    /// hex identity of the showcase's primary author (the user-* components
    /// resolve it through `LiveProfileMap` reactively). No profile fields
    /// are baked in here — kind:0 metadata arrives via the kernel snapshot.
    pub fn live_initial(primary_pubkey: &str) -> Self {
        Self::build(primary_pubkey)
    }

    /// Test gallery data uses the same real Nostr references as live mode.
    /// The user-* component tests drive profiles through `LiveProfileMap`,
    /// never through this struct.
    #[cfg(test)]
    pub fn render_test_data() -> Self {
        Self::build(showcase_pubkey())
    }

    /// Build trees that contain only real Nostr references. Relay-provided
    /// fields arrive through `claimed_events`, `claimed_profiles`, and
    /// `mention_profiles`; this initializer does not invent event bodies,
    /// authors, media, profile names, or profile pictures.
    fn build(primary_pubkey: &str) -> Self {
        let mention_uri = format!("nostr:{}", showcase_npub());
        let note_nevent = note_nevent();
        let article_naddr = article_naddr();
        let highlight_nevent = highlight_nevent();

        Self {
            primary_pubkey: primary_pubkey.to_string(),
            avatar_image: None,
            avatar_image_compact: None,
            media_images: Vec::new(),
            content_core: content_example("relay note reference", note_nevent)
                .expect("real note reference must tokenize"),
            content_minimal: content_example("relay profile mention", &mention_uri)
                .expect("real profile reference must tokenize"),
            content_view: content_example(
                "relay note content",
                &format!("relay note {note_nevent}"),
            )
            .expect("real note reference must tokenize"),
            content_mention_chip: content_example("relay profile mention", &mention_uri)
                .expect("real profile reference must tokenize"),
            content_media_grid: content_example("relay article media", article_naddr)
                .expect("real article reference must tokenize"),
            content_quote_card: content_example("relay quote card", note_nevent)
                .expect("real note reference must tokenize"),
            embed_article: content_example("Embedded Article (kind:30023)", article_naddr)
                .expect("real article reference must tokenize"),
            embed_profile: content_example("Inline Profile Mention (kind:0)", &mention_uri)
                .expect("real profile reference must tokenize"),
            embed_note: content_example("Embedded Note (kind:1)", note_nevent)
                .expect("real note reference must tokenize"),
            embed_highlight: content_example("Embedded Highlight (kind:9802)", highlight_nevent)
                .expect("real highlight reference must tokenize"),
        }
    }
}

fn content_example(title: &str, content: &str) -> Result<ContentExample, String> {
    let tree = tree_for_content(content)?;
    Ok(ContentExample {
        scenario_id: title.to_string(),
        title: title.to_string(),
        tree,
        render_data: ContentRenderData::default(),
    })
}

fn tree_for_content(content: &str) -> Result<ContentTreeWire, String> {
    let wire = tokenize_with_kind(content, &[], RenderMode::Auto, 1).to_wire();
    let value =
        serde_json::to_value(wire).map_err(|e| format!("content tree encode failed: {e}"))?;
    ContentTreeWire::from_value(&value).ok_or_else(|| "content tree decode failed".to_string())
}
