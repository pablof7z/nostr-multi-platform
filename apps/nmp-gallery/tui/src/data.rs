//! Initial gallery view state — what every page needs at frame zero.
//!
//! Live-only (ADR-0034 / M16): there is no fixture mode, no hardcoded
//! embed envelopes, no `load_images: bool` knob that toggles fakery. The
//! gallery boots, the kernel runs through cold-start, and this module
//! turns the resulting `LiveFacts` (profiles + thread/author items
//! resolved from kind:0 / kind:1 / etc.) into the content trees and
//! `ContentRenderData` the renderer needs to draw the first frame.
//!
//! Embedded events do NOT live here. The renderer is frontend-driven:
//! when `NostrContentView` hits an `EventRef(uri)` it calls
//! `sink.claim(uri, …)`, the kernel fetches (cache or relay), and the
//! resolved envelopes flow through `EmbedHostState` (see `embed_host.rs`).
//! Renderers consume the host's envelope map at render time, not a
//! static field on `ContentExample`.

use std::{
    io::{IsTerminal, Read},
    time::Duration,
};

use nmp_content::{tokenize_with_kind, RenderMode};
use nmp_core::display::{short_hex, short_npub, to_npub};
use ratatui::layout::Size;
use ratatui_image::{picker::Picker, picker::ProtocolType, protocol::Protocol, Resize};
use serde_json::{json, Map, Value};

use crate::{
    content_render_data::ContentRenderData,
    content_tree_wire::ContentTreeWire,
    live::{LiveFacts, LiveItem, LiveProfile},
    profile_wire::ProfileWire,
};

/// The naddr the embed-article showcase references in its synthesized
/// content string. The renderer encounters this URI inside the content
/// tree, calls `host.claim(uri, ...)`, the kernel fetches the kind:30023,
/// and `EmbedHostState` decodes it into an `ArticleProjection`. Defining
/// it here (rather than inline) makes the showcase reproducible — anyone
/// running the gallery TUI claims THIS naddr.
pub const ARTICLE_NADDR: &str = "nostr:naddr1qvzqqqr4gupzqmjxss3dld622uu8q25gywum9qtg4w4cv4064jmg20xsac2aam5nqy6xsar5wpen5te0v3jhyemfva5jucm0d5hnyvpjxchnqve0xgcz7argv5kkjmn5v4exuet594kx2en594kk2tcqz36xsefdd9h8getjdejhgttvv4n8gttdv55zqsmp";

/// pablof7z kind:1 note "grok cli is INSANELY bad, jesus" — verified on
/// wss://relay.primal.net via `nak req` (event id 276d69d6…).
pub const NOTE_NEVENT: &str = "nostr:nevent1qqszwmtf6mfdeq6g62st0fnjg4grjzwutfq967awvx5zfhpzfcga0pqpzemhxue69uhhyetvv9ujuurjd9kkzmpwdejhgq3ql2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqlxqxcq";

/// pablof7z kind:9802 highlight "Vibe-coding is what brought me back to
/// programming" — verified on wss://relay.primal.net (event id 4fb59c3c…).
pub const HIGHLIGHT_NEVENT: &str = "nostr:nevent1qqsyldvu8s4pwha9vqqvur0ht4d2gj0e7u3kmguv9hpf0thuk5prjwspzemhxue69uhhyetvv9ujuurjd9kkzmpwdejhgq3ql2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afq2dlzvt";

pub struct GalleryData {
    pub primary_profile: ProfileWire,
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

impl GalleryData {
    /// Build the initial gallery view state from the kernel's cold-start
    /// fetch. The renderer (driven by snapshot pushes) later updates the
    /// embed host with resolved kind:30023 / kind:9802 / kind:1 envelopes
    /// — those do NOT live on this struct.
    ///
    /// `load_images` controls the synchronous avatar/media HTTP fetches.
    /// Set `false` for non-TTY environments (CI dump-lines, tests).
    pub fn from_live(facts: &LiveFacts, load_images: bool) -> Result<Self, String> {
        let primary = &facts.primary_profile;
        let secondary = &facts.mention_profile;

        let avatar_images = if load_images {
            avatar_protocols(primary)
        } else {
            AvatarProtocols::default()
        };
        let media_images = if load_images {
            media_protocols(&[&facts.media_item, &facts.quote_target_item])
        } else {
            Vec::new()
        };

        let mention_profiles = [(secondary, Some(facts.mention_profile_uri.as_str()))];
        let quote_events = [(
            &facts.quote_target_item,
            &facts.quote_target_profile,
            Some(facts.quote_event_uri.as_str()),
        )];

        // Embed-showcase content strings. The renderer turns the embedded
        // bech32 URIs into `EventRef` tokens, claims via the sink, and the
        // resolved envelopes arrive through `EmbedHostState`. We do NOT
        // synthesize envelopes here.
        let article_item = synth_item(
            &primary.pubkey,
            1,
            &format!("hey, check out my article {ARTICLE_NADDR} I hope you enjoy it!"),
        );
        let mention_uri_for_embed = facts.mention_profile_uri.clone();
        let profile_embed_item = synth_item(
            &primary.pubkey,
            1,
            &format!(
                "met {mention_uri_for_embed} at a nostr conference last week, brilliant mind"
            ),
        );
        let note_embed_item = synth_item(
            &primary.pubkey,
            1,
            &format!(
                "this is a great point {NOTE_NEVENT} what do you think?"
            ),
        );
        let highlight_embed_item = synth_item(
            &primary.pubkey,
            1,
            &format!("found this interesting {HIGHLIGHT_NEVENT}"),
        );

        Ok(Self {
            primary_profile: profile_wire(primary),
            secondary_profile: profile_wire(secondary),
            avatar_image: avatar_images.large,
            avatar_image_compact: avatar_images.compact,
            media_images,
            content_core: content_example(
                &facts.mention_item,
                "live mention tree",
                &mention_profiles,
                &[],
            )?,
            content_minimal: content_example(
                &facts.mention_item,
                "live minimal mention",
                &mention_profiles,
                &[],
            )?,
            content_view: content_example(&facts.media_item, "live image content", &[], &[])?,
            content_mention_chip: content_example(
                &facts.mention_item,
                "live mention chip",
                &mention_profiles,
                &[],
            )?,
            content_media_grid: content_example(
                &facts.media_item,
                "live media grid",
                &[],
                &[],
            )?,
            content_quote_card: content_example(
                &facts.quote_source_item,
                "live quote card",
                &[],
                &quote_events,
            )?,
            embed_article: content_example(
                &article_item,
                "Embedded Article (kind:30023)",
                &[],
                &[],
            )?,
            embed_profile: content_example(
                &profile_embed_item,
                "Inline Profile Mention (via mention chip)",
                &mention_profiles,
                &[],
            )?,
            embed_note: content_example(
                &note_embed_item,
                "Embedded Note (kind:1)",
                &[],
                &[],
            )?,
            embed_highlight: content_example(
                &highlight_embed_item,
                "Embedded Highlight (kind:9802)",
                &[],
                &[],
            )?,
        })
    }

    /// Synthetic `LiveFacts`-equivalent for unit tests that need a
    /// `GalleryData` without spinning up the kernel. The profiles and
    /// items use deterministic test-only pubkeys/event-ids; no embed
    /// envelopes are synthesized (the renderer-triggered path is
    /// exercised by `embed_host::tests`, not by `render::tests`).
    #[cfg(test)]
    pub(crate) fn render_test_data() -> Self {
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
        let mention_item = LiveItem {
            id: "4444444444444444444444444444444444444444444444444444444444444444".to_string(),
            author_pubkey: author_pubkey.to_string(),
            kind: 1,
            content: format!("hello {mention_uri}"),
            content_preview: String::new(),
            created_at: 1,
        };
        let quote_source = LiveItem {
            id: "5555555555555555555555555555555555555555555555555555555555555555".to_string(),
            author_pubkey: author_pubkey.to_string(),
            kind: 1,
            content: format!("look {quote_uri}"),
            content_preview: String::new(),
            created_at: 2,
        };
        let quote_target = LiveItem {
            id: quote_id.to_string(),
            author_pubkey: author_pubkey.to_string(),
            kind: 1,
            content: "Quoted event body from render data".to_string(),
            content_preview: String::new(),
            created_at: 3,
        };

        let facts = LiveFacts {
            primary_profile: quote_author.clone(),
            mention_profile: resolved_profile,
            quote_target_profile: quote_author,
            mention_item,
            media_item: quote_source.clone(),
            quote_source_item: quote_source,
            quote_target_item: quote_target,
            mention_profile_uri: mention_uri,
            quote_event_uri: quote_uri,
        };
        Self::from_live(&facts, false).expect("test data is valid")
    }
}

#[derive(Default)]
struct AvatarProtocols {
    large: Option<Protocol>,
    compact: Option<Protocol>,
}

fn avatar_protocols(profile: &LiveProfile) -> AvatarProtocols {
    let Some(url) = profile.picture_url.as_deref() else {
        return AvatarProtocols::default();
    };
    if !std::io::stdout().is_terminal() {
        return AvatarProtocols::default();
    }
    let Some(image) = fetch_image(url) else {
        return AvatarProtocols::default();
    };
    let mut picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    if std::env::var("ITERM_SESSION_ID").is_ok() {
        picker.set_protocol_type(ProtocolType::Iterm2);
    }
    AvatarProtocols {
        large: picker
            .new_protocol(image.clone(), Size::new(18, 9), Resize::Fit(None))
            .ok(),
        compact: picker
            .new_protocol(image, Size::new(12, 4), Resize::Fit(None))
            .ok(),
    }
}

fn fetch_image(url: &str) -> Option<image::DynamicImage> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(12))
        .build();
    let response = agent.get(url).call().ok()?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(4 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    image::load_from_memory(&bytes).ok()
}

fn media_protocols(items: &[&LiveItem]) -> Vec<MediaProtocol> {
    if !std::io::stdout().is_terminal() {
        return Vec::new();
    }
    let mut urls = Vec::new();
    for item in items {
        for url in media_urls_for_item(item) {
            if !urls.contains(&url) {
                urls.push(url);
            }
        }
    }
    let mut picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    if std::env::var("ITERM_SESSION_ID").is_ok() {
        picker.set_protocol_type(ProtocolType::Iterm2);
    }
    urls.into_iter()
        .filter_map(|url| {
            let image = fetch_image(&url)?;
            let protocol = picker
                .new_protocol(image, Size::new(30, 8), Resize::Fit(None))
                .ok()?;
            Some(MediaProtocol { url, protocol })
        })
        .collect()
}

fn media_urls_for_item(item: &LiveItem) -> Vec<String> {
    tree_for_item(item)
        .map(|tree| {
            let mut out = Vec::new();
            for node in tree.nodes {
                match node {
                    crate::content_tree_wire::WireNode::Media { urls, .. } => out.extend(urls),
                    crate::content_tree_wire::WireNode::Image { src: Some(src), .. } => {
                        out.push(src)
                    }
                    _ => {}
                }
            }
            out
        })
        .unwrap_or_default()
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
