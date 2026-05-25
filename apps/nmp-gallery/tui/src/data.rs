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
    live::{LiveFacts, LiveGallerySource, LiveItem, LiveProfile},
    profile_wire::ProfileWire,
};

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
    pub fn load(load_images: bool) -> Result<Self, String> {
        let facts = LiveGallerySource::new(Duration::from_secs(45)).load()?;
        Self::from_live_facts(facts, load_images)
    }

    fn from_live_facts(facts: LiveFacts, load_images: bool) -> Result<Self, String> {
        let avatar_images = if load_images {
            avatar_protocols(&facts.primary_profile)
        } else {
            AvatarProtocols::default()
        };
        let media_images = if load_images {
            media_protocols(&[&facts.media_item, &facts.quote_target_item])
        } else {
            Vec::new()
        };
        let primary_profile = profile_wire(&facts.primary_profile);
        let secondary_profile = profile_wire(&facts.mention_profile);
        let mention_profiles = [(
            &facts.mention_profile,
            Some(facts.mention_profile_uri.as_str()),
        )];
        let quote_events = [(
            &facts.quote_target_item,
            &facts.quote_target_profile,
            Some(facts.quote_event_uri.as_str()),
        )];

        Ok(Self {
            primary_profile,
            secondary_profile,
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
            content_media_grid: content_example(&facts.media_item, "live media grid", &[], &[])?,
            content_quote_card: content_example(
                &facts.quote_source_item,
                "live quote card",
                &[],
                &quote_events,
            )?,
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
        let profile = LiveProfile {
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
            content_preview: "hello profile".to_string(),
            created_at: 1,
        };
        let quote_source = LiveItem {
            id: "5555555555555555555555555555555555555555555555555555555555555555".to_string(),
            author_pubkey: author_pubkey.to_string(),
            kind: 1,
            content: format!("look {quote_uri}"),
            content_preview: "look quote".to_string(),
            created_at: 2,
        };
        let quote_target = LiveItem {
            id: quote_id.to_string(),
            author_pubkey: author_pubkey.to_string(),
            kind: 1,
            content: "Quoted event body from render data".to_string(),
            content_preview: "Quoted event body from render data".to_string(),
            created_at: 3,
        };
        let facts = LiveFacts {
            primary_profile: quote_author.clone(),
            mention_profile: profile,
            quote_target_profile: quote_author,
            mention_item,
            media_item: quote_source.clone(),
            quote_source_item: quote_source,
            quote_target_item: quote_target,
            mention_profile_uri: mention_uri,
            quote_event_uri: quote_uri,
        };
        Self::from_live_facts(facts, false).expect("test data is valid")
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

fn profile_value(profile: &LiveProfile) -> Value {
    json!({
        "pubkey": profile.pubkey,
        "display_name": profile.display_label(),
        "npub": to_npub(&profile.pubkey),
        "picture_url": profile.picture_url,
    })
}
