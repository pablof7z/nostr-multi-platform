use std::{
    collections::BTreeMap,
    io::{IsTerminal, Read},
    time::Duration,
};

use nmp_content::{
    embed_projection::{
        ArticleProjection, EmbedKindProjection, EmbeddedEventEnvelope, HighlightProjection,
        RenderContextWire, ShortNoteProjection,
    },
    tokenize_with_kind, RenderMode,
};
use nmp_core::display::{short_hex, short_npub, to_npub};
use ratatui::layout::Size;
use ratatui_image::{picker::Picker, picker::ProtocolType, protocol::Protocol, Resize};
use serde_json::{json, Map, Value};

use crate::{
    content_render_data::ContentRenderData,
    content_tree_wire::ContentTreeWire,
    profile_wire::ProfileWire,
};

const PRIMARY_PUBKEY: &str = "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";
const SECONDARY_PUBKEY: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const MENTION_EVENT_ID: &str = "caef905a1e1520fd6621b56364cca823c262327a32ac063b4ff0435f41aa7660";
const MEDIA_EVENT_ID: &str = "c2ee64b0371f290edf66fc797598b2d307aa79192f6d6e0bf5344cf81104029b";
const QUOTE_SOURCE_EVENT_ID: &str =
    "2df88accbf264b10f47809abcf9d32b4146b035a5a197c9ff30e45ac010d5368";
const ARTICLE_NADDR: &str = "nostr:naddr1qvzqqqr4gupzqmjxss3dld622uu8q25gywum9qtg4w4cv4064jmg20xsac2aam5nqy6xsar5wpen5te0v3jhyemfva5jucm0d5hnyvpjxchnqve0xgcz7argv5kkjmn5v4exuet594kx2en594kk2tcqz36xsefdd9h8getjdejhgttvv4n8gttdv55zqsmp";

struct FixtureProfile {
    pubkey: String,
    display_name: Option<String>,
    picture_url: Option<String>,
    nip05: Option<String>,
    about: Option<String>,
}

struct FixtureItem {
    id: String,
    author_pubkey: String,
    kind: u32,
    content: String,
    created_at: u64,
}

impl FixtureProfile {
    fn display_label(&self) -> String {
        self.display_name
            .as_deref()
            .filter(|n| !n.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.pubkey.clone())
    }
}

impl FixtureItem {
    fn preview(&self) -> String {
        self.content.replace('\n', " ").chars().take(180).collect()
    }
}

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
    pub embedded_events: BTreeMap<String, EmbeddedEventEnvelope>,
}

pub struct MediaProtocol {
    pub url: String,
    pub protocol: Protocol,
}

impl GalleryData {
    pub fn load(load_images: bool) -> Result<Self, String> {
        let primary = FixtureProfile {
            pubkey: PRIMARY_PUBKEY.to_string(),
            display_name: Some("Pablo".to_string()),
            picture_url: Some("https://pablof7z.com/avatar.png".to_string()),
            nip05: Some("pablo@pablof7z.com".to_string()),
            about: Some("Building NMP and other stuff.".to_string()),
        };
        let secondary = FixtureProfile {
            pubkey: SECONDARY_PUBKEY.to_string(),
            display_name: Some("fiatjaf".to_string()),
            picture_url: None,
            nip05: Some("fiatjaf@fiatjaf.com".to_string()),
            about: Some("The guy who made nostr.".to_string()),
        };

        let secondary_npub_uri = format!("nostr:{}", to_npub(SECONDARY_PUBKEY));
        let mention_note_uri = note_uri(MENTION_EVENT_ID);

        let mention_item = FixtureItem {
            id: MENTION_EVENT_ID.to_string(),
            author_pubkey: PRIMARY_PUBKEY.to_string(),
            kind: 1,
            content: format!(
                "{secondary_npub_uri} shipped another elegant NIP today — one of the most creative minds in the protocol space #nostr"
            ),
            created_at: 1716000000,
        };
        let media_item = FixtureItem {
            id: MEDIA_EVENT_ID.to_string(),
            author_pubkey: PRIMARY_PUBKEY.to_string(),
            kind: 1,
            content: "The view is incredible \u{1f305} https://images.unsplash.com/photo-1506905925346-21bda4d32df4?w=800 #photography #nostr".to_string(),
            created_at: 1716001000,
        };
        let quote_source_item = FixtureItem {
            id: QUOTE_SOURCE_EVENT_ID.to_string(),
            author_pubkey: PRIMARY_PUBKEY.to_string(),
            kind: 1,
            content: format!("this resonated with me {mention_note_uri} what do you think?"),
            created_at: 1716002000,
        };

        let avatar_images = if load_images {
            avatar_protocols(&primary)
        } else {
            AvatarProtocols::default()
        };
        let media_images = if load_images {
            media_protocols(&[&media_item])
        } else {
            Vec::new()
        };

        let mention_profiles = [(&secondary, Some(secondary_npub_uri.as_str()))];

        Ok(Self {
            primary_profile: profile_wire(&primary),
            secondary_profile: profile_wire(&secondary),
            avatar_image: avatar_images.large,
            avatar_image_compact: avatar_images.compact,
            media_images,
            content_core: content_example(
                &mention_item,
                "fixture mention tree",
                &mention_profiles,
                &[],
            )?,
            content_minimal: content_example(
                &mention_item,
                "fixture minimal mention",
                &mention_profiles,
                &[],
            )?,
            content_view: content_example(&media_item, "fixture image content", &[], &[])?,
            content_mention_chip: content_example(
                &mention_item,
                "fixture mention chip",
                &mention_profiles,
                &[],
            )?,
            content_media_grid: content_example(&media_item, "fixture media grid", &[], &[])?,
            content_quote_card: content_example(
                &quote_source_item,
                "fixture quote card",
                &[],
                &[],
            )?,
            embed_article: {
                let content_tree =
                    tokenize_with_kind("Long-form article body here.", &[], RenderMode::Auto, 30023)
                        .to_wire();
                let projection = EmbedKindProjection::Article(ArticleProjection {
                    id: MENTION_EVENT_ID.to_string(),
                    author_pubkey: PRIMARY_PUBKEY.to_string(),
                    author_display_name: Some("Pablo".to_string()),
                    author_picture_url: Some("https://pablof7z.com/avatar.png".to_string()),
                    created_at: 1716000000,
                    title: Some("Kind-Dispatch Content Rendering (ADR-0034)".to_string()),
                    summary: Some(
                        "How NMP routes embedded Nostr events to typed platform renderers."
                            .to_string(),
                    ),
                    hero_image_url: None,
                    d_tag: "kind-dispatch".to_string(),
                    content_tree,
                });
                embed_example(
                    MENTION_EVENT_ID,
                    "Embedded Article (kind:30023)",
                    &format!("hey, check out my article {ARTICLE_NADDR} I hope you enjoy it!"),
                    // keyed by the naddr URI string (primary_id for naddr = author pubkey which
                    // we don't know without decoding; the uri fallback in envelope_for catches it)
                    ARTICLE_NADDR,
                    projection,
                )?
            },
            embed_profile: static_embed_example(
                MENTION_EVENT_ID,
                "Inline Profile Mention (via mention chip)",
                &format!(
                    "met {secondary_npub_uri} at a nostr conference last week, brilliant mind"
                ),
                1,
            )?,
            embed_note: {
                let content_tree = tokenize_with_kind(
                    "This is the quoted note body rendered via ShortNoteProjection.",
                    &[],
                    RenderMode::Auto,
                    1,
                )
                .to_wire();
                let projection = EmbedKindProjection::ShortNote(ShortNoteProjection {
                    id: MENTION_EVENT_ID.to_string(),
                    author_pubkey: SECONDARY_PUBKEY.to_string(),
                    author_display_name: Some("fiatjaf".to_string()),
                    author_picture_url: None,
                    created_at: 1716000000,
                    content_tree,
                    media_urls: vec![],
                });
                embed_example(
                    MENTION_EVENT_ID,
                    "Embedded Note (kind:1)",
                    &format!("this is a great point {mention_note_uri} what do you think?"),
                    &mention_note_uri,
                    projection,
                )?
            },
            embed_highlight: {
                let projection = EmbedKindProjection::Highlight(HighlightProjection {
                    id: QUOTE_SOURCE_EVENT_ID.to_string(),
                    author_pubkey: SECONDARY_PUBKEY.to_string(),
                    author_display_name: Some("fiatjaf".to_string()),
                    created_at: 1716002000,
                    highlighted_text:
                        "The simplest protocol wins because protocol simplicity is leverage."
                            .to_string(),
                    source_event_id: None,
                    source_event_addr: None,
                    source_url: Some("https://fiatjaf.com".to_string()),
                    context: None,
                });
                let quote_note_uri = note_uri(MENTION_EVENT_ID);
                embed_example(
                    MENTION_EVENT_ID,
                    "Embedded Highlight (kind:9802)",
                    &format!("found this interesting {quote_note_uri}"),
                    &quote_note_uri,
                    projection,
                )?
            },
        })
    }

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

        let resolved_profile = FixtureProfile {
            pubkey: referenced_pubkey.to_string(),
            display_name: Some("Resolved Profile".to_string()),
            picture_url: Some("https://example.invalid/profile.png".to_string()),
            nip05: Some("resolved.example".to_string()),
            about: Some("Test-only resolved profile".to_string()),
        };
        let quote_author = FixtureProfile {
            pubkey: author_pubkey.to_string(),
            display_name: Some("Quoted Author".to_string()),
            picture_url: None,
            nip05: None,
            about: None,
        };
        let mention_item = FixtureItem {
            id: "4444444444444444444444444444444444444444444444444444444444444444".to_string(),
            author_pubkey: author_pubkey.to_string(),
            kind: 1,
            content: format!("hello {mention_uri}"),
            created_at: 1,
        };
        let quote_source = FixtureItem {
            id: "5555555555555555555555555555555555555555555555555555555555555555".to_string(),
            author_pubkey: author_pubkey.to_string(),
            kind: 1,
            content: format!("look {quote_uri}"),
            created_at: 2,
        };
        let quote_target = FixtureItem {
            id: quote_id.to_string(),
            author_pubkey: author_pubkey.to_string(),
            kind: 1,
            content: "Quoted event body from render data".to_string(),
            created_at: 3,
        };

        let mention_profiles = [(&resolved_profile, Some(mention_uri.as_str()))];
        let quote_events = [(&quote_target, &quote_author, Some(quote_uri.as_str()))];

        let embed_note_tree =
            tokenize_with_kind(&quote_target.content, &[], RenderMode::Auto, 1).to_wire();
        let embed_note_projection = EmbedKindProjection::ShortNote(ShortNoteProjection {
            id: quote_target.id.clone(),
            author_pubkey: quote_target.author_pubkey.clone(),
            author_display_name: quote_author.display_name.clone(),
            author_picture_url: quote_author.picture_url.clone(),
            created_at: quote_target.created_at,
            content_tree: embed_note_tree,
            media_urls: vec![],
        });
        let embed_article_tree =
            tokenize_with_kind(&quote_target.content, &[], RenderMode::Auto, 30023).to_wire();
        let embed_article_projection = EmbedKindProjection::Article(ArticleProjection {
            id: quote_target.id.clone(),
            author_pubkey: quote_target.author_pubkey.clone(),
            author_display_name: quote_author.display_name.clone(),
            author_picture_url: quote_author.picture_url.clone(),
            created_at: quote_target.created_at,
            title: Some("Gallery Article Demo (kind:30023)".to_string()),
            summary: Some(quote_target.content.clone()),
            hero_image_url: None,
            d_tag: "gallery-demo".to_string(),
            content_tree: embed_article_tree,
        });
        let embed_highlight_projection = EmbedKindProjection::Highlight(HighlightProjection {
            id: quote_target.id.clone(),
            author_pubkey: quote_target.author_pubkey.clone(),
            author_display_name: quote_author.display_name.clone(),
            created_at: quote_target.created_at,
            highlighted_text: quote_target.content.clone(),
            source_event_id: Some(quote_target.id.clone()),
            source_event_addr: None,
            source_url: None,
            context: None,
        });

        let make_embed_item = |content: String| FixtureItem {
            id: quote_target.id.clone(),
            author_pubkey: author_pubkey.to_string(),
            kind: 1,
            content,
            created_at: 0,
        };

        Self {
            primary_profile: profile_wire(&quote_author),
            secondary_profile: profile_wire(&resolved_profile),
            avatar_image: None,
            avatar_image_compact: None,
            media_images: Vec::new(),
            content_core: content_example(&mention_item, "live mention tree", &mention_profiles, &[])
                .expect("test data is valid"),
            content_minimal: content_example(
                &mention_item,
                "live minimal mention",
                &mention_profiles,
                &[],
            )
            .expect("test data is valid"),
            content_view: content_example(&quote_source, "live image content", &[], &[])
                .expect("test data is valid"),
            content_mention_chip: content_example(
                &mention_item,
                "live mention chip",
                &mention_profiles,
                &[],
            )
            .expect("test data is valid"),
            content_media_grid: content_example(&quote_source, "live media grid", &[], &[])
                .expect("test data is valid"),
            content_quote_card: content_example(
                &quote_source,
                "live quote card",
                &[],
                &quote_events,
            )
            .expect("test data is valid"),
            embed_article: embed_example_from_fixture(
                &make_embed_item(format!("check out this article: {quote_uri}")),
                "Embedded Article (kind:30023)",
                &quote_uri,
                embed_article_projection,
            )
            .expect("test data is valid"),
            embed_profile: content_example(
                &mention_item,
                "Inline Profile Mention (via mention chip)",
                &mention_profiles,
                &[],
            )
            .expect("test data is valid"),
            embed_note: embed_example_from_fixture(
                &make_embed_item(format!("this is a great point: {quote_uri}")),
                "Embedded Note (kind:1)",
                &quote_uri,
                embed_note_projection,
            )
            .expect("test data is valid"),
            embed_highlight: embed_example_from_fixture(
                &make_embed_item(format!("interesting highlight: {quote_uri}")),
                "Embedded Highlight (kind:9802)",
                &quote_uri,
                embed_highlight_projection,
            )
            .expect("test data is valid"),
        }
    }
}

#[derive(Default)]
struct AvatarProtocols {
    large: Option<Protocol>,
    compact: Option<Protocol>,
}

fn avatar_protocols(profile: &FixtureProfile) -> AvatarProtocols {
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

fn media_protocols(items: &[&FixtureItem]) -> Vec<MediaProtocol> {
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

fn media_urls_for_item(item: &FixtureItem) -> Vec<String> {
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

fn profile_wire(profile: &FixtureProfile) -> ProfileWire {
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

fn content_example(
    item: &FixtureItem,
    title: &str,
    profiles: &[(&FixtureProfile, Option<&str>)],
    events: &[(&FixtureItem, &FixtureProfile, Option<&str>)],
) -> Result<ContentExample, String> {
    let tree = tree_for_item(item)?;
    Ok(ContentExample {
        scenario_id: short_hex(&item.id),
        title: title.to_string(),
        tree,
        render_data: render_data_for(profiles, events)?,
        embedded_events: BTreeMap::new(),
    })
}

fn static_embed_example(
    id: &str,
    title: &str,
    content: &str,
    kind: u32,
) -> Result<ContentExample, String> {
    let item = FixtureItem {
        id: id.to_string(),
        author_pubkey: PRIMARY_PUBKEY.to_string(),
        kind,
        content: content.to_string(),
        created_at: 1716000000,
    };
    Ok(ContentExample {
        scenario_id: short_hex(id),
        title: title.to_string(),
        tree: tree_for_item(&item)?,
        render_data: ContentRenderData::default(),
        embedded_events: BTreeMap::new(),
    })
}

fn embed_example(
    event_id: &str,
    title: &str,
    content: &str,
    uri_key: &str,
    projection: EmbedKindProjection,
) -> Result<ContentExample, String> {
    let item = FixtureItem {
        id: event_id.to_string(),
        author_pubkey: PRIMARY_PUBKEY.to_string(),
        kind: 1,
        content: content.to_string(),
        created_at: 0,
    };
    let tree = tree_for_item(&item)?;
    let envelope = EmbeddedEventEnvelope {
        uri: uri_key.to_string(),
        primary_id: event_id.to_string(),
        render_context: RenderContextWire {
            depth: 0,
            max_depth: 4,
            visited: vec![],
        },
        projection,
        collapsed: false,
        collapse_reason: None,
    };
    let mut embedded_events = BTreeMap::new();
    embedded_events.insert(event_id.to_string(), envelope.clone());
    embedded_events.insert(uri_key.to_string(), envelope);
    Ok(ContentExample {
        scenario_id: short_hex(event_id),
        title: title.to_string(),
        tree,
        render_data: ContentRenderData::default(),
        embedded_events,
    })
}

#[cfg(test)]
fn embed_example_from_fixture(
    item: &FixtureItem,
    title: &str,
    uri_key: &str,
    projection: EmbedKindProjection,
) -> Result<ContentExample, String> {
    embed_example(&item.id, title, &item.content, uri_key, projection)
}

fn tree_for_item(item: &FixtureItem) -> Result<ContentTreeWire, String> {
    let value = tree_value_for_item(item)?;
    ContentTreeWire::from_value(&value).ok_or_else(|| "content tree decode failed".to_string())
}

fn tree_value_for_item(item: &FixtureItem) -> Result<Value, String> {
    let wire = tokenize_with_kind(&item.content, &[], RenderMode::Auto, item.kind).to_wire();
    serde_json::to_value(wire).map_err(|e| format!("content tree encode failed: {e}"))
}

fn render_data_for(
    profiles: &[(&FixtureProfile, Option<&str>)],
    events: &[(&FixtureItem, &FixtureProfile, Option<&str>)],
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
            "content_preview": event.preview(),
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

fn profile_value(profile: &FixtureProfile) -> Value {
    json!({
        "pubkey": profile.pubkey,
        "display_name": profile.display_label(),
        "npub": to_npub(&profile.pubkey),
        "picture_url": profile.picture_url,
    })
}

fn note_uri(event_id: &str) -> String {
    format!(
        "nostr:{}",
        nmp_core::nip19::format(&nmp_core::nip19::Nip19Entity::Note(event_id.to_string()))
            .expect("note id formats")
    )
}
