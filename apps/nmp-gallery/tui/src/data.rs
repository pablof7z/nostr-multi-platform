//! Initial gallery view state — what every page needs at frame zero.
//!
//! Live-only (ADR-0034 / M16): there is no fixture mode, no hardcoded
//! embed envelopes, no `load_images: bool` knob that toggles fakery. The
//! gallery boots, the kernel runs, and this module provides the
//! synthetic content trees and `ContentRenderData` the renderer needs
//! to draw the first frame without blocking on network fetches.
//!
//! Embedded events do NOT live here. The renderer is frontend-driven:
//! when `NostrContentView` hits an `EventRef(uri)` it calls
//! `sink.claim(uri, …)`, the kernel fetches (cache or relay), and the
//! resolved envelopes flow through `EmbedHostState` (see `embed_host.rs`).
//! Renderers consume the host's envelope map at render time, not a
//! static field on `ContentExample`.

use std::collections::HashMap;

use nmp_content::{tokenize_with_kind, RenderMode};
use nmp_core::display::{short_hex, short_npub, to_npub};
use ratatui_image::protocol::Protocol;
use serde_json::{json, Map, Value};

use crate::{
    content_render_data::ContentRenderData,
    content_tree_wire::ContentTreeWire,
    profile_wire::ProfileWire,
};

/// The naddr the embed-article showcase references in its synthesized
/// content string. The renderer encounters this URI inside the content
/// tree, calls `host.claim(uri, ...)`, the kernel fetches the kind:30023,
/// and `EmbedHostState` decodes it into an `ArticleProjection`. Defining
/// it here (rather than inline) makes the showcase reproducible — anyone
/// running the gallery TUI claims THIS naddr.
/// Gigi's "What's left of the internet?" (kind:30023, d="the-internet-left-me")
/// — the canonical kind-dispatch demo. Loads cleanly when the kernel queries
/// relay.primal.net before NIP-65 hydration kicks in (visible via NMP_WIRE_LOG).
/// After NIP-65 routes the OneshotApi REQ exclusively to Gigi's outbox relays
/// (atlas.nostr.land, eden.nostr.land) which don't carry it, so a cold cache
/// post-hydration sees the loading placeholder. The renderer + projection
/// path is exercised in either case (smoke verifies cache-hit → resolved
/// envelope in 700ms).
pub const ARTICLE_NADDR: &str = "nostr:naddr1qvzqqqr4gupzqmjxss3dld622uu8q25gywum9qtg4w4cv4064jmg20xsac2aam5nqy6xsar5wpen5te0v3jhyemfva5jucm0d5hnyvpjxchnqve0xgcz7argv5kkjmn5v4exuet594kx2en594kk2tcqz36xsefdd9h8getjdejhgttvv4n8gttdv55zqsmp";

/// pablof7z kind:1 note "grok cli is INSANELY bad, jesus" — verified on
/// wss://relay.primal.net via `nak req` (event id 276d69d6…).
pub const NOTE_NEVENT: &str = "nostr:nevent1qqszwmtf6mfdeq6g62st0fnjg4grjzwutfq967awvx5zfhpzfcga0pqpzemhxue69uhhyetvv9ujuurjd9kkzmpwdejhgq3ql2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqlxqxcq";

/// pablof7z kind:9802 highlight "Vibe-coding is what brought me back to
/// programming" — verified on wss://relay.primal.net (event id 4fb59c3c…).
pub const HIGHLIGHT_NEVENT: &str = "nostr:nevent1qqsyldvu8s4pwha9vqqvur0ht4d2gj0e7u3kmguv9hpf0thuk5prjwspzemhxue69uhhyetvv9ujuurjd9kkzmpwdejhgq3ql2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afq2dlzvt";

pub struct GalleryData {
    /// Hex pubkey of the showcase's primary author. The user-* components
    /// resolve their `ProfileWire` reactively from `LiveProfileMap` at
    /// render time — `GalleryData` carries the *identity* (a pubkey), never
    /// a snapshot of profile fields. Kind:0 metadata flows in through the
    /// kernel snapshot, not through this struct's initialization.
    pub primary_pubkey: String,
    pub secondary_profile: ProfileWire,
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
/// field-by-field copying and no fake placeholder profile.
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
    /// Two projections feed this:
    /// - `projections.mention_profiles` — `{ pubkey: { pubkey, display_name,
    ///   picture_url } }`. The lightweight per-mention payload (no nip05 /
    ///   about). Establishes a profile entry for every author the kernel has
    ///   kind:0 for among the visible items.
    /// - `projections.author_view.profile` — the full `ProfileCard`
    ///   (`pubkey, npub, display_name, picture_url, nip05, about,
    ///   has_profile`). Richer than `mention_profiles`, so when present and
    ///   `has_profile == true` it *overrides* any same-pubkey entry from
    ///   `mention_profiles`.
    pub fn update_from_snapshot(&mut self, snapshot: &Value) {
        let Some(projections) = snapshot.get("projections") else {
            return;
        };

        if let Some(mention_profiles) = projections
            .get("mention_profiles")
            .and_then(Value::as_object)
        {
            for (pubkey, payload) in mention_profiles {
                let display_name = string_field(payload, "display_name");
                let picture_url = string_field(payload, "picture_url");
                let wire = self.entry_for(pubkey);
                wire.display_name = display_name;
                wire.picture_url = picture_url;
            }
        }

        // author_view.profile wins: it carries the full field set and only
        // appears once a kind:0 has actually been received (has_profile).
        if let Some(profile) = projections
            .get("author_view")
            .and_then(|av| av.get("profile"))
        {
            let is_real = profile
                .get("has_profile")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if is_real {
                if let Some(pubkey) = string_field(profile, "pubkey") {
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
        }
    }

    /// Resolve the `ProfileWire` for `pubkey`, falling back to a name-less
    /// wire (pubkey + npub + npub_short, everything else `None`) when no
    /// kind:0 has arrived yet. The fallback carries NO fake display name —
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

#[derive(Clone)]
pub struct LiveProfile {
    pub pubkey: String,
    pub display_name: Option<String>,
    pub picture_url: Option<String>,
    pub nip05: Option<String>,
    pub about: Option<String>,
}

#[derive(Clone)]
pub struct LiveItem {
    pub id: String,
    pub author_pubkey: String,
    pub kind: u32,
    pub content: String,
    pub content_preview: String,
    pub created_at: u64,
}

impl LiveProfile {
    pub fn display_label(&self) -> String {
        self.display_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.pubkey.clone())
    }
}

impl LiveItem {
    pub fn preview(&self) -> String {
        if self.content_preview.trim().is_empty() {
            self.content.replace('\n', " ").chars().take(180).collect()
        } else {
            self.content_preview.clone()
        }
    }
}

impl GalleryData {
    /// Initial gallery state for the live program. `primary_pubkey` is the
    /// hex identity of the showcase's primary author (the user-* components
    /// resolve it through `LiveProfileMap` reactively). No profile fields
    /// are baked in here — kind:0 metadata arrives via the kernel snapshot.
    pub fn live_initial(primary_pubkey: &str) -> Self {
        Self::build(primary_pubkey)
    }

    /// Synthetic gallery data for tests. Identical content trees to
    /// `live_initial`; the primary author identity is a deterministic
    /// placeholder pubkey. The user-* component tests drive profiles
    /// through `LiveProfileMap`, never through this struct.
    #[cfg(test)]
    pub fn render_test_data() -> Self {
        Self::build("2222222222222222222222222222222222222222222222222222222222222222")
    }

    /// Build the synthetic content trees and the primary-author identity.
    /// No network fetches — all content items are deterministic
    /// placeholders. Embeds are triggered reactively by the renderer via
    /// `EventClaimSink` as usual; profiles resolve through `LiveProfileMap`.
    fn build(primary_pubkey: &str) -> Self {
        let referenced_pubkey = "1111111111111111111111111111111111111111111111111111111111111111";
        let author_pubkey = "2222222222222222222222222222222222222222222222222222222222222222";
        let quote_id = "3333333333333333333333333333333333333333333333333333333333333333";
        let mention_uri = format!("nostr:{}", to_npub(referenced_pubkey));
        let quote_uri = format!(
            "nostr:{}",
            nmp_core::nip19::format(&nmp_core::nip19::Nip19Entity::Note(quote_id.to_string()))
                .expect("note id formats")
        );

        let resolved_profile = LiveProfile {
            pubkey: referenced_pubkey.to_string(),
            display_name: Some("Resolved Profile".to_string()),
            picture_url: Some("https://example.invalid/profile.png".to_string()),
            nip05: Some("resolved.example".to_string()),
            about: Some("Test-only resolved profile".to_string()),
        };
        let quote_author = LiveProfile {
            pubkey: author_pubkey.to_string(),
            display_name: Some("Quoted Author".to_string()),
            picture_url: None,
            nip05: None,
            about: None,
        };
        let primary_profile = LiveProfile {
            pubkey: author_pubkey.to_string(),
            display_name: Some("Primary Author".to_string()),
            picture_url: Some("https://example.invalid/avatar.png".to_string()),
            nip05: Some("primary.example".to_string()),
            about: Some("Primary author for gallery showcase".to_string()),
        };

        let mention_item = LiveItem {
            id: "4444444444444444444444444444444444444444444444444444444444444444".to_string(),
            author_pubkey: author_pubkey.to_string(),
            kind: 1,
            content: format!("hello {mention_uri}"),
            content_preview: String::new(),
            created_at: 1,
        };
        let media_item = LiveItem {
            id: "6666666666666666666666666666666666666666666666666666666666666666".to_string(),
            author_pubkey: author_pubkey.to_string(),
            kind: 1,
            content: "Check out this image https://example.invalid/image1.png and this one https://example.invalid/image2.png".to_string(),
            content_preview: String::new(),
            created_at: 2,
        };
        let quote_source = LiveItem {
            id: "5555555555555555555555555555555555555555555555555555555555555555".to_string(),
            author_pubkey: author_pubkey.to_string(),
            kind: 1,
            content: format!("look {quote_uri}"),
            content_preview: String::new(),
            created_at: 3,
        };
        let quote_target = LiveItem {
            id: quote_id.to_string(),
            author_pubkey: author_pubkey.to_string(),
            kind: 1,
            content: "Quoted event body from render data".to_string(),
            content_preview: String::new(),
            created_at: 4,
        };

        let mention_profiles = [(&resolved_profile, Some(mention_uri.as_str()))];
        let quote_events = [(&quote_target, &quote_author, Some(quote_uri.as_str()))];

        let article_item = synth_item(
            &primary_profile.pubkey,
            1,
            &format!("hey, check out my article {ARTICLE_NADDR} I hope you enjoy it!"),
        );
        let profile_embed_item = synth_item(
            &primary_profile.pubkey,
            1,
            &format!("met {mention_uri} at a nostr conference last week, brilliant mind"),
        );
        let note_embed_item = synth_item(
            &primary_profile.pubkey,
            1,
            &format!("this is a great point {NOTE_NEVENT} what do you think?"),
        );
        let highlight_embed_item = synth_item(
            &primary_profile.pubkey,
            1,
            &format!("found this interesting {HIGHLIGHT_NEVENT}"),
        );

        Self {
            primary_pubkey: primary_pubkey.to_string(),
            secondary_profile: profile_wire(&resolved_profile),
            avatar_image: None,
            avatar_image_compact: None,
            media_images: Vec::new(),
            content_core: content_example(
                &mention_item, "live mention tree", &mention_profiles, &[]
            )
                .expect("test data valid"),
            content_minimal: content_example(
                &mention_item, "live minimal mention", &mention_profiles, &[]
            )
                .expect("test data valid"),
            content_view: content_example(&media_item, "live image content", &[], &[])
                .expect("test data valid"),
            content_mention_chip: content_example(
                &mention_item, "live mention chip", &mention_profiles, &[]
            )
                .expect("test data valid"),
            content_media_grid: content_example(&media_item, "live media grid", &[], &[])
                .expect("test data valid"),
            content_quote_card: content_example(&quote_source, "live quote card", &[], &quote_events)
                .expect("test data valid"),
            embed_article: content_example(
                &article_item, "Embedded Article (kind:30023)", &[], &[]
            )
                .expect("test data valid"),
            embed_profile: content_example(
                &profile_embed_item, "Inline Profile Mention (via mention chip)", &mention_profiles, &[]
            )
                .expect("test data valid"),
            embed_note: content_example(&note_embed_item, "Embedded Note (kind:1)", &[], &[])
                .expect("test data valid"),
            embed_highlight: content_example(
                &highlight_embed_item, "Embedded Highlight (kind:9802)", &[], &[]
            )
                .expect("test data valid"),
        }
    }
}

fn profile_wire(profile: &LiveProfile) -> ProfileWire {
    ProfileWire {
        pubkey: profile.pubkey.clone(),
        display_name: profile.display_name.clone(),
        about: profile.about.clone(),
        picture_url: profile.picture_url.clone(),
        nip05: profile.nip05.clone(),
        npub: to_npub(&profile.pubkey),
        npub_short: short_npub(&profile.pubkey),
    }
}

/// Build a synthetic `LiveItem` for embed-showcase content strings that
/// reference real bech32 URIs the renderer will claim. The `id` is
/// deterministic-from-content so the same showcase rebuild always produces
/// the same scenario_id badge.
fn synth_item(author_pubkey: &str, kind: u32, content: &str) -> LiveItem {
    LiveItem {
        id: deterministic_id(content),
        author_pubkey: author_pubkey.to_string(),
        kind,
        content: content.to_string(),
        content_preview: String::new(),
        created_at: 0,
    }
}

fn deterministic_id(content: &str) -> String {
    // 64-hex string derived from content; not a real event id, just a
    // stable label for `scenario_id` and the renderer's per-event keys.
    let mut h: u64 = 1469598103934665603;
    for b in content.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(1099511628211);
    }
    format!("{h:016x}{h:016x}{h:016x}{h:016x}")
}

fn content_example(
    item: &LiveItem,
    title: &str,
    profiles: &[(&LiveProfile, Option<&str>)],
    events: &[(&LiveItem, &LiveProfile, Option<&str>)],
) -> Result<ContentExample, String> {
    let tree = tree_for_item(item)?;
    Ok(ContentExample {
        scenario_id: short_hex(&item.id),
        title: title.to_string(),
        tree,
        render_data: render_data_for(profiles, events)?,
    })
}

fn tree_for_item(item: &LiveItem) -> Result<ContentTreeWire, String> {
    let value = tree_value_for_item(item)?;
    ContentTreeWire::from_value(&value).ok_or_else(|| "content tree decode failed".to_string())
}

fn tree_value_for_item(item: &LiveItem) -> Result<Value, String> {
    let wire = tokenize_with_kind(&item.content, &[], RenderMode::Auto, item.kind).to_wire();
    serde_json::to_value(wire).map_err(|e| format!("content tree encode failed: {e}"))
}

fn render_data_for(
    profiles: &[(&LiveProfile, Option<&str>)],
    events: &[(&LiveItem, &LiveProfile, Option<&str>)],
) -> Result<ContentRenderData, String> {
    let mut profile_map = Map::new();
    for (profile, uri) in profiles {
        let value = profile_value(profile);
        profile_map.insert(profile.pubkey.clone(), value.clone());
        if let Some(uri) = uri {
            profile_map.insert((*uri).to_string(), value);
        }
    }

    let mut event_map = Map::new();
    for (event, author, uri) in events {
        let content_tree = tree_value_for_item(event)?;
        let value = json!({
            "id": event.id,
            "author_pubkey": event.author_pubkey,
            "author_display_name": author.display_label(),
            "author_npub": to_npub(&event.author_pubkey),
            "kind": event.kind,
            "created_at": event.created_at,
            "content_preview": preview_of(event),
            "content_tree": content_tree,
        });
        event_map.insert(event.id.clone(), value.clone());
        if let Some(uri) = uri {
            event_map.insert((*uri).to_string(), value);
        }
    }

    let value = Value::Object(Map::from_iter([
        ("profiles".to_string(), Value::Object(profile_map)),
        ("events".to_string(), Value::Object(event_map)),
    ]));
    Ok(ContentRenderData::from_value(Some(&value)))
}

fn profile_value(profile: &LiveProfile) -> Value {
    json!({
        "pubkey": profile.pubkey,
        "display_name": profile.display_label(),
        "npub": to_npub(&profile.pubkey),
        "picture_url": profile.picture_url,
    })
}

fn preview_of(item: &LiveItem) -> String {
    if item.content_preview.trim().is_empty() {
        item.content.replace('\n', " ").chars().take(180).collect()
    } else {
        item.content_preview.clone()
    }
}
