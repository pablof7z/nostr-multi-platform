use std::collections::{BTreeMap, BTreeSet};

use nmp_core::display::{short_hex, short_npub};
use serde_json::Value;

use crate::snapshot::MentionProfile;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimelineContent {
    pub text: String,
    pub media: Vec<TimelineMedia>,
}

impl TimelineContent {
    pub fn from_card(card: &Value, profiles: &BTreeMap<String, MentionProfile>) -> Self {
        if let Some(content) = from_content_tree(card, profiles) {
            return content;
        }
        from_plain_content(&string_field(card, "content"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineMedia {
    pub kind: TimelineMediaKind,
    pub url: String,
    pub alt: Option<String>,
}

impl TimelineMedia {
    pub fn label(&self) -> &'static str {
        self.kind.label()
    }

    pub fn compact_display(&self) -> String {
        let trimmed = self
            .url
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let display = if trimmed.chars().count() > 72 {
            let head = trimmed.chars().take(69).collect::<String>();
            format!("{head}...")
        } else {
            trimmed.to_string()
        };
        match self.alt.as_deref().filter(|alt| !alt.trim().is_empty()) {
            Some(alt) => format!("{}: {display}", alt.trim()),
            None => display,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TimelineMediaKind {
    Image,
    Video,
    Audio,
}

impl TimelineMediaKind {
    fn label(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
        }
    }
}

struct ContentBuilder<'a> {
    nodes: &'a [Value],
    profiles: &'a BTreeMap<String, MentionProfile>,
    text: String,
    media: Vec<TimelineMedia>,
    seen_media: BTreeSet<(TimelineMediaKind, String)>,
}

impl<'a> ContentBuilder<'a> {
    fn new(nodes: &'a [Value], profiles: &'a BTreeMap<String, MentionProfile>) -> Self {
        Self {
            nodes,
            profiles,
            text: String::new(),
            media: Vec::new(),
            seen_media: BTreeSet::new(),
        }
    }

    fn into_content(self) -> TimelineContent {
        TimelineContent {
            text: preview(&compact_whitespace(&self.text)),
            media: self.media,
        }
    }

    fn append_node(&mut self, index: u64) {
        let Some(node) = self.nodes.get(index as usize) else {
            return;
        };
        match node.get("kind").and_then(Value::as_str) {
            Some("text") => self.push_text(&string_field(node, "text")),
            Some("mention") => self.push_text(&mention_label(node, self.profiles)),
            Some("event_ref") => self.push_text(&reference_label(node, "note")),
            Some("hashtag") => self.push_text(&format!("#{}", string_field(node, "tag"))),
            Some("url") => self.append_url(&string_field(node, "url")),
            Some("media") => self.append_media_node(node),
            Some("emoji") => self.push_text(&format!(":{}:", string_field(node, "shortcode"))),
            Some("invoice") => self.push_text("[invoice]"),
            Some("heading") | Some("paragraph") | Some("block_quote") | Some("emphasis")
            | Some("strong") => {
                self.append_children(node);
                self.push_text(" ");
            }
            Some("code_block") => self.push_text(&string_field(node, "body")),
            Some("list") => self.append_list(node),
            Some("inline_code") => self.push_text(&string_field(node, "code")),
            Some("link") => self.append_link(node),
            Some("image") => self.append_image_node(node),
            Some("soft_break") | Some("hard_break") | Some("rule") => self.push_text(" "),
            Some("placeholder") | None => {}
            Some(_) => self.append_children(node),
        }
    }

    fn append_children(&mut self, node: &Value) {
        for child in node
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
        {
            self.append_node(child);
        }
    }

    fn append_list(&mut self, node: &Value) {
        for item in node
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for child in item
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
            {
                self.append_node(child);
            }
            self.push_text(" ");
        }
    }

    fn append_link(&mut self, node: &Value) {
        let before = self.text.len();
        self.append_children(node);
        if self.text.len() == before {
            self.append_url(&optional_string(node, "href").unwrap_or_default());
        }
    }

    fn append_url(&mut self, url: &str) {
        if let Some(kind) = media_kind_from_url(url) {
            self.add_media(kind, url.to_string(), None);
        } else {
            self.push_text(url);
        }
    }

    fn append_media_node(&mut self, node: &Value) {
        let kind = match string_field(node, "media_kind").as_str() {
            "Image" | "image" => TimelineMediaKind::Image,
            "Video" | "video" => TimelineMediaKind::Video,
            "Audio" | "audio" => TimelineMediaKind::Audio,
            _ => return,
        };
        for url in node
            .get("urls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            self.add_media(kind, url.to_string(), None);
        }
    }

    fn append_image_node(&mut self, node: &Value) {
        let Some(src) = optional_string(node, "src") else {
            return;
        };
        self.add_media(
            TimelineMediaKind::Image,
            src,
            optional_string(node, "alt").or_else(|| optional_string(node, "title")),
        );
    }

    fn add_media(&mut self, kind: TimelineMediaKind, url: String, alt: Option<String>) {
        if url.trim().is_empty() {
            return;
        }
        let key = (kind, url.clone());
        if self.seen_media.insert(key) {
            self.media.push(TimelineMedia { kind, url, alt });
        }
    }

    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.text.push_str(text);
    }
}

fn from_content_tree(
    card: &Value,
    profiles: &BTreeMap<String, MentionProfile>,
) -> Option<TimelineContent> {
    let tree = card.get("content_tree")?;
    let nodes = tree.get("nodes").and_then(Value::as_array)?;
    let roots = tree.get("roots").and_then(Value::as_array)?;
    let mut builder = ContentBuilder::new(nodes, profiles);
    for root in roots.iter().filter_map(Value::as_u64) {
        builder.append_node(root);
    }
    let mut content = builder.into_content();
    if content.text.is_empty() && content.media.is_empty() {
        content = from_plain_content(&string_field(card, "content"));
    }
    Some(content)
}

fn from_plain_content(content: &str) -> TimelineContent {
    let mut media = Vec::new();
    let mut seen_media = BTreeSet::new();
    let mut words = Vec::new();
    for word in content.split_whitespace() {
        let cleaned = word.trim_matches(|c: char| matches!(c, ',' | ')' | ']' | '.'));
        if let Some(kind) = media_kind_from_url(cleaned) {
            if seen_media.insert((kind, cleaned.to_string())) {
                media.push(TimelineMedia {
                    kind,
                    url: cleaned.to_string(),
                    alt: None,
                });
            }
        } else {
            words.push(word);
        }
    }
    TimelineContent {
        text: preview(&words.join(" ")),
        media,
    }
}

fn mention_label(node: &Value, profiles: &BTreeMap<String, MentionProfile>) -> String {
    let uri = node.get("uri").unwrap_or(&Value::Null);
    let primary_id = string_field(uri, "primary_id");
    if string_field(uri, "kind") == "profile" && is_hex_pubkey_64(&primary_id) {
        let display = profiles
            .get(&primary_id)
            .map(|profile| profile.display.trim())
            .filter(|display| !display.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| short_npub(&primary_id));
        return format!("@{}", display.trim_start_matches('@'));
    }
    reference_label(node, "nostr")
}

fn reference_label(node: &Value, fallback_prefix: &str) -> String {
    let uri = node.get("uri").unwrap_or(&Value::Null);
    let primary_id = string_field(uri, "primary_id");
    if !primary_id.is_empty() {
        return format!("{fallback_prefix}:{}", short_hex(&primary_id));
    }
    string_field(uri, "uri")
}

fn media_kind_from_url(url: &str) -> Option<TimelineMediaKind> {
    let lower = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return None;
    }
    if matches!(
        extension(&lower).as_deref(),
        Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "avif")
    ) {
        return Some(TimelineMediaKind::Image);
    }
    if matches!(extension(&lower).as_deref(), Some("mp4" | "mov" | "webm")) {
        return Some(TimelineMediaKind::Video);
    }
    if matches!(
        extension(&lower).as_deref(),
        Some("mp3" | "wav" | "m4a" | "ogg")
    ) {
        return Some(TimelineMediaKind::Audio);
    }
    None
}

fn extension(url: &str) -> Option<String> {
    url.rsplit_once('.').map(|(_, ext)| ext.to_string())
}

fn is_hex_pubkey_64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn compact_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn preview(compact: &str) -> String {
    if compact.chars().count() <= 96 {
        compact.to_string()
    } else {
        let preview = compact.chars().take(95).collect::<String>();
        format!("{preview}...")
    }
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_profile_mentions_from_content_tree() {
        let pubkey = "a".repeat(64);
        let mut profiles = BTreeMap::new();
        profiles.insert(
            pubkey.clone(),
            MentionProfile {
                display: "branie".to_string(),
                picture_url: String::new(),
                avatar_initials: "BR".to_string(),
                avatar_color: "FF00FF".to_string(),
            },
        );
        let card = serde_json::json!({
            "content": "when nostr:npub1raw posted",
            "content_tree": {
                "nodes": [
                    {"kind": "text", "text": "when "},
                    {"kind": "mention", "uri": {"uri": "nostr:npub1...", "kind": "profile", "primary_id": pubkey, "relays": []}},
                    {"kind": "text", "text": " posted"}
                ],
                "roots": [0, 1, 2],
                "mode": "plaintext"
            }
        });

        let content = TimelineContent::from_card(&card, &profiles);

        assert_eq!(content.text, "when @branie posted");
    }

    #[test]
    fn extracts_media_nodes_without_rendering_raw_urls_as_body() {
        let card = serde_json::json!({
            "content": "checkout https://example.com/cat.jpg",
            "content_tree": {
                "nodes": [
                    {"kind": "text", "text": "checkout "},
                    {"kind": "media", "media_kind": "Image", "urls": ["https://example.com/cat.jpg"]}
                ],
                "roots": [0, 1],
                "mode": "plaintext"
            }
        });

        let content = TimelineContent::from_card(&card, &BTreeMap::new());

        assert_eq!(content.text, "checkout");
        assert_eq!(content.media.len(), 1);
        assert_eq!(content.media[0].kind, TimelineMediaKind::Image);
    }

    #[test]
    fn plain_content_fallback_extracts_image_urls() {
        let content = from_plain_content("look https://example.com/a.webp");

        assert_eq!(content.text, "look");
        assert_eq!(content.media[0].url, "https://example.com/a.webp");
    }
}
