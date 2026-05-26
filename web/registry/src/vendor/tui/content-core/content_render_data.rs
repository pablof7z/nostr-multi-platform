use std::collections::BTreeMap;

use serde_json::Value;

use super::content_tree_wire::{ContentTreeWire, WireUri};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentRenderData {
    profiles: BTreeMap<String, ContentProfileRenderData>,
    events: BTreeMap<String, ContentEventRenderData>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentProfileRenderData {
    pub pubkey: String,
    pub display_name: Option<String>,
    pub npub: Option<String>,
    pub picture_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentEventRenderData {
    pub id: String,
    pub author_pubkey: String,
    pub author_display_name: Option<String>,
    pub author_npub: Option<String>,
    pub kind: u64,
    pub created_at: u64,
    pub content_preview: String,
    pub content_tree: Option<ContentTreeWire>,
}

impl ContentRenderData {
    pub fn from_value(value: Option<&Value>) -> Self {
        let Some(value) = value else {
            return Self::default();
        };
        Self {
            profiles: map_values(value.get("profiles"), ContentProfileRenderData::from_value),
            events: map_values(value.get("events"), ContentEventRenderData::from_value),
        }
    }

    pub fn profile_for(&self, uri: &WireUri) -> Option<&ContentProfileRenderData> {
        self.profiles
            .get(&uri.primary_id)
            .or_else(|| self.profiles.get(&uri.uri))
    }

    pub fn event_for(&self, uri: &WireUri) -> Option<&ContentEventRenderData> {
        self.events
            .get(&uri.primary_id)
            .or_else(|| self.events.get(&uri.uri))
    }

    pub fn media_urls(&self) -> Vec<String> {
        let mut out = Vec::new();
        for event in self.events.values() {
            if let Some(tree) = event.content_tree.as_ref() {
                for url in tree.media_urls() {
                    if !out.iter().any(|existing| existing == &url) {
                        out.push(url);
                    }
                }
            }
        }
        out
    }
}

impl ContentProfileRenderData {
    fn from_value(key: &str, value: &Value) -> Option<Self> {
        let display = value.get("display").unwrap_or(value);
        Some(Self {
            pubkey: string(value, "pubkey")
                .or_else(|| string(display, "pubkey"))
                .unwrap_or_else(|| key.to_string()),
            display_name: string(display, "name")
                .or_else(|| string(value, "profile_name"))
                .or_else(|| string(value, "display_name")),
            npub: string(display, "npub").or_else(|| string(value, "npub")),
            picture_url: string(display, "picture_url")
                .or_else(|| string(value, "profile_picture"))
                .or_else(|| string(value, "picture_url")),
        })
    }

    pub fn label(&self) -> &str {
        self.display_name
            .as_deref()
            .or(self.npub.as_deref())
            .unwrap_or(&self.pubkey)
    }
}

impl ContentEventRenderData {
    fn from_value(key: &str, value: &Value) -> Option<Self> {
        let author_display = value.get("author_display").unwrap_or(value);
        Some(Self {
            id: string(value, "id").unwrap_or_else(|| key.to_string()),
            author_pubkey: string(value, "author_pubkey").unwrap_or_default(),
            author_display_name: string(author_display, "name")
                .or_else(|| string(value, "author_display_name")),
            author_npub: string(author_display, "npub").or_else(|| string(value, "author_npub")),
            kind: value.get("kind").and_then(Value::as_u64).unwrap_or(1),
            created_at: value.get("created_at").and_then(Value::as_u64).unwrap_or(0),
            content_preview: string(value, "content_preview").unwrap_or_default(),
            content_tree: value
                .get("content_tree")
                .and_then(ContentTreeWire::from_value),
        })
    }

    pub fn author_label(&self) -> &str {
        self.author_display_name
            .as_deref()
            .or(self.author_npub.as_deref())
            .unwrap_or(&self.author_pubkey)
    }
}

fn map_values<T>(
    value: Option<&Value>,
    parse: impl Fn(&str, &Value) -> Option<T>,
) -> BTreeMap<String, T> {
    let mut out = BTreeMap::new();
    let Some(value) = value else {
        return out;
    };
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            if let Some(parsed) = parse(key, value) {
                out.insert(key.clone(), parsed);
            }
        }
    } else if let Some(array) = value.as_array() {
        for value in array {
            let key = string(value, "pubkey")
                .or_else(|| string(value, "id"))
                .or_else(|| string(value, "uri"))
                .unwrap_or_default();
            if !key.is_empty() {
                if let Some(parsed) = parse(&key, value) {
                    out.insert(key, parsed);
                }
            }
        }
    }
    out
}

fn string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
