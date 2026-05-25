//! `nmp-nip18` — NIP-18 repost decoding primitives.
//!
//! This crate owns generic repost wire interpretation. It does not render UI,
//! choose relay policy, or depend on any app crate.

use nmp_core::substrate::KernelEvent;
use serde::Deserialize;

/// NIP-18 repost event kind.
pub const KIND_REPOST: u32 = 6;

/// Decoded inner event embedded in a kind:6 repost `content` field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedEvent {
    pub id: String,
    pub author: String,
    pub kind: u32,
    pub created_at: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

/// Decoded kind:6 repost record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepostRecord {
    pub event_id: String,
    pub author: String,
    pub created_at: u64,
    pub target_event_id: Option<String>,
    pub embedded_event: Option<EmbeddedEvent>,
}

/// Decode a [`KernelEvent`] as a NIP-18 repost.
///
/// Returns `None` for every non-kind:6 event. A kind:6 event with only an
/// `e` tag and no embedded event is still a repost record; consumers can render
/// a placeholder while the target is unresolved.
#[must_use]
pub fn try_from_kernel_event(event: &KernelEvent) -> Option<RepostRecord> {
    if event.kind != KIND_REPOST {
        return None;
    }

    let embedded_event = parse_embedded_event(&event.content);
    let target_event_id = first_event_tag(&event.tags)
        .or_else(|| embedded_event.as_ref().map(|inner| inner.id.clone()));

    Some(RepostRecord {
        event_id: event.id.clone(),
        author: event.author.clone(),
        created_at: event.created_at,
        target_event_id,
        embedded_event,
    })
}

#[derive(Deserialize)]
struct EmbeddedEventWire {
    id: String,
    pubkey: String,
    kind: u32,
    created_at: u64,
    #[serde(default)]
    tags: Vec<Vec<String>>,
    content: String,
}

fn parse_embedded_event(raw: &str) -> Option<EmbeddedEvent> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let wire: EmbeddedEventWire = serde_json::from_str(trimmed).ok()?;
    Some(EmbeddedEvent {
        id: wire.id,
        author: wire.pubkey,
        kind: wire.kind,
        created_at: wire.created_at,
        tags: wire.tags,
        content: wire.content,
    })
}

fn first_event_tag(tags: &[Vec<String>]) -> Option<String> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|name| name == "e") {
            tag.get(1).filter(|id| !id.is_empty()).cloned()
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: u32, content: &str, tags: Vec<Vec<&str>>) -> KernelEvent {
        KernelEvent {
            id: "repost".to_string(),
            author: "alice".to_string(),
            kind,
            created_at: 42,
            tags: tags
                .into_iter()
                .map(|tag| tag.into_iter().map(str::to_string).collect())
                .collect(),
            content: content.to_string(),
        }
    }

    #[test]
    fn rejects_non_repost_kind() {
        assert!(try_from_kernel_event(&event(1, "hello", vec![])).is_none());
    }

    #[test]
    fn decodes_repost_with_event_tag_only() {
        let record =
            try_from_kernel_event(&event(KIND_REPOST, "", vec![vec!["e", "target"]])).unwrap();

        assert_eq!(record.target_event_id.as_deref(), Some("target"));
        assert!(record.embedded_event.is_none());
    }

    #[test]
    fn decodes_embedded_event_payload() {
        let content = r#"{
            "id":"inner",
            "pubkey":"bob",
            "kind":1,
            "created_at":123,
            "tags":[["p","alice"]],
            "content":"hello #nostr",
            "sig":"ignored"
        }"#;
        let record = try_from_kernel_event(&event(KIND_REPOST, content, vec![])).unwrap();
        let inner = record.embedded_event.as_ref().unwrap();

        assert_eq!(record.target_event_id.as_deref(), Some("inner"));
        assert_eq!(inner.author, "bob");
        assert_eq!(inner.kind, 1);
        assert_eq!(inner.tags, vec![vec!["p".to_string(), "alice".to_string()]]);
        assert_eq!(inner.content, "hello #nostr");
    }

    #[test]
    fn malformed_embedded_json_still_decodes_repost_record() {
        let record = try_from_kernel_event(&event(
            KIND_REPOST,
            r#"{"content":"missing required event fields"}"#,
            vec![vec!["e", "target"]],
        ))
        .unwrap();

        assert_eq!(record.target_event_id.as_deref(), Some("target"));
        assert!(record.embedded_event.is_none());
    }
}
