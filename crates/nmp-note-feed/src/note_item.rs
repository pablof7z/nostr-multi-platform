use nmp_content::{tokenize_with_kind, ContentTreeWire, RenderMode, WireNode, WireNostrUriKind};
use nmp_core::substrate::KernelEvent;
use nmp_feed::{CardAuthors, FeedCard};
use nmp_nip18::try_from_kernel_event as try_from_repost_event;
use serde::{Deserialize, Serialize};

use crate::card_payload::RenderPayload;

/// One concrete row payload for note-feed surfaces.
///
/// This is a feed-composition type, not a NIP-01 protocol primitive. It carries
/// raw event ids/pubkeys/content plus structural content parsing. Presentation
/// data such as profile display, previews, counts, and social action state are
/// owned by the component or concept read that asks for them.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct NoteFeedItem {
    pub id: String,
    pub author_pubkey: String,
    pub kind: u32,
    pub created_at: u64,
    pub content: String,
    pub content_tree: ContentTreeWire,
    /// Genuine network relays this note was received from — never the
    /// local-publish self-echo (`KernelEvent::received_from_relays` filters
    /// it out). A shell's "Received from N relays" affordance must count only
    /// this list; a note is not relay-received just because it was published.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relay_provenance: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reposted_by: Option<RepostAttribution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hosted_group: Option<HostedGroupContext>,
}

/// Typed group context attached by hosted-group feed sessions.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostedGroupContext {
    pub host_relay_url: String,
    pub local_id: String,
}

/// Attribution for a row surfaced by a NIP-18 repost wrapper.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RepostAttribution {
    pub author_pubkey: String,
    pub note_created_at: u64,
}

impl CardAuthors for NoteFeedItem {
    fn rendered_author_keys(&self) -> Vec<String> {
        let mut keys = vec![self.author_pubkey.clone()];
        if let Some(repost) = &self.reposted_by {
            keys.push(repost.author_pubkey.clone());
        }
        keys
    }
}

impl FeedCard for NoteFeedItem {
    fn feed_created_at(&self) -> u64 {
        self.created_at
    }

    fn feed_event_refs(&self) -> Vec<String> {
        self.content_tree
            .nodes
            .iter()
            .filter_map(|node| match node {
                WireNode::EventRef { uri } if uri.kind == WireNostrUriKind::Event => {
                    Some(uri.primary_id.clone())
                }
                _ => None,
            })
            .collect()
    }
}

impl NoteFeedItem {
    #[must_use]
    pub fn from_event_for_op_feed(event: &KernelEvent, target: Option<&KernelEvent>) -> Self {
        Self::from_event_for_op_feed_with_hosted_group(event, target, None)
    }

    #[must_use]
    pub fn from_event_for_op_feed_with_hosted_group(
        event: &KernelEvent,
        target: Option<&KernelEvent>,
        hosted_group: Option<HostedGroupContext>,
    ) -> Self {
        let Some(repost) = try_from_repost_event(event) else {
            let mut item = Self::from_event(event);
            item.hosted_group = hosted_group;
            return item;
        };

        let target_id = repost
            .target_event_id
            .clone()
            .unwrap_or_else(|| event.id.clone());

        let (mut item, note_created_at) = if let Some(target_event) = target {
            (Self::from_event(target_event), target_event.created_at)
        } else if let Some(inner) = repost.embedded_event.as_ref() {
            (Self::from_event(event), inner.created_at)
        } else {
            (Self::from_event(event), event.created_at)
        };

        item.id = target_id;
        item.reposted_by = Some(RepostAttribution {
            author_pubkey: event.author.clone(),
            note_created_at,
        });
        item.created_at = event.created_at;
        item.hosted_group = hosted_group;
        item
    }

    fn from_event(event: &KernelEvent) -> Self {
        let render_payload = RenderPayload::from_event(event);
        let content_tree = tokenize_with_kind(
            &render_payload.content,
            &render_payload.tags,
            RenderMode::Auto,
            render_payload.kind,
        )
        .to_wire();
        let display_author = render_payload.author.as_deref().unwrap_or(&event.author);
        let reposted_by = render_payload.repost_attribution(&event.author);
        Self {
            id: event.id.clone(),
            author_pubkey: display_author.to_string(),
            kind: render_payload.kind,
            created_at: event.created_at,
            content: render_payload.content,
            content_tree,
            relay_provenance: event.received_from_relays(),
            reposted_by,
            hosted_group: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_event(relay_provenance: Vec<String>) -> KernelEvent {
        KernelEvent {
            id: "1".repeat(64),
            author: "2".repeat(64),
            kind: 1,
            created_at: 100,
            tags: Vec::new(),
            content: "hello".to_string(),
            relay_provenance,
        }
    }

    #[test]
    fn note_feed_item_reports_zero_relays_for_local_publish_only_provenance() {
        // chirp#116: while offline, a freshly published note's only provenance
        // is the app's own `local://publish` echo. The feed row must not
        // claim relay receipt for a note that never left the device.
        let event = note_event(vec!["local://publish".to_string()]);
        let item = NoteFeedItem::from_event_for_op_feed(&event, None);
        assert!(
            item.relay_provenance.is_empty(),
            "local-publish self-echo must never count as a relay receipt"
        );
    }

    #[test]
    fn note_feed_item_keeps_real_relays_alongside_local_publish_echo() {
        let event = note_event(vec![
            "local://publish".to_string(),
            "wss://relay.example".to_string(),
        ]);
        let item = NoteFeedItem::from_event_for_op_feed(&event, None);
        assert_eq!(
            item.relay_provenance,
            vec!["wss://relay.example".to_string()]
        );
    }
}
