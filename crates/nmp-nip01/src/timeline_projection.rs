//! Reusable NIP-10 modular timeline projection with render-card payloads.
//!
//! `Nip10ModularTimelineView` groups event ids into blocks. Most native
//! shells also need the per-event render metadata in the same pushed snapshot,
//! so this projection owns the generic card cache beside the view state.

use std::collections::HashMap;
use std::sync::Mutex;

use nmp_content::{tokenize_with_kind, ContentTreeWire, RenderMode};
use nmp_core::substrate::{KernelEvent, ViewContext, ViewModule};
use nmp_core::KernelEventObserver;
use nmp_threading::TimelineBlock;
use serde::{Deserialize, Serialize};

use crate::meta_timeline::{
    ModularTimelinePayload, ModularTimelineSpec, ModularTimelineState, Nip10ModularTimelineView,
};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TimelineEventCard {
    pub id: String,
    pub author_pubkey: String,
    pub kind: u32,
    pub created_at: u64,
    pub content: String,
    pub content_tree: ContentTreeWire,
}

impl From<&KernelEvent> for TimelineEventCard {
    fn from(event: &KernelEvent) -> Self {
        let content_tree =
            tokenize_with_kind(&event.content, &event.tags, RenderMode::Auto, event.kind).to_wire();
        Self {
            id: event.id.clone(),
            author_pubkey: event.author.clone(),
            kind: event.kind,
            created_at: event.created_at,
            content: event.content.clone(),
            content_tree,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModularTimelineSnapshot {
    pub blocks: Vec<TimelineBlock>,
    pub cards: Vec<TimelineEventCard>,
}

impl ModularTimelineSnapshot {
    pub fn empty() -> Self {
        Self {
            blocks: Vec::new(),
            cards: Vec::new(),
        }
    }
}

pub struct ModularTimelineProjection {
    inner: Mutex<Inner>,
}

struct Inner {
    state: ModularTimelineState,
    cards: HashMap<String, TimelineEventCard>,
}

impl ModularTimelineProjection {
    pub fn new(spec: ModularTimelineSpec) -> Self {
        let ctx = ViewContext::default();
        let (state, _payload) = Nip10ModularTimelineView::open(&ctx, spec);
        Self {
            inner: Mutex::new(Inner {
                state,
                cards: HashMap::new(),
            }),
        }
    }

    pub fn snapshot(&self) -> ModularTimelineSnapshot {
        let Ok(inner) = self.inner.lock() else {
            return ModularTimelineSnapshot::empty();
        };
        let ctx = ViewContext::default();
        let payload: ModularTimelinePayload =
            Nip10ModularTimelineView::snapshot(&ctx, &inner.state);
        ModularTimelineSnapshot {
            blocks: payload.blocks,
            cards: inner.cards.values().cloned().collect(),
        }
    }
}

impl KernelEventObserver for ModularTimelineProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let ctx = ViewContext::default();
        inner
            .cards
            .insert(event.id.clone(), TimelineEventCard::from(event));
        Nip10ModularTimelineView::on_event_inserted(&ctx, &mut inner.state, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_content::{WireNode, WireNostrUriKind};
    use nmp_core::nip19::encode_npub;
    use nmp_threading::{ModulePolicy, TimelineBlock};
    use std::sync::Arc;

    fn spec() -> ModularTimelineSpec {
        ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            authors: None,
            policy: ModulePolicy::default(),
        }
    }

    fn note(id: &str, ts: u64, tags: Vec<Vec<String>>) -> KernelEvent {
        note_with_content(id, ts, tags, id)
    }

    fn note_with_content(
        id: &str,
        ts: u64,
        tags: Vec<Vec<String>>,
        content: &str,
    ) -> KernelEvent {
        KernelEvent {
            id: id.into(),
            author: "auth".into(),
            kind: 1,
            created_at: ts,
            tags,
            content: content.into(),
        }
    }

    fn reply_to(id: &str, ts: u64, root: &str, parent: &str) -> KernelEvent {
        note(
            id,
            ts,
            vec![
                vec!["e".into(), root.into(), "".into(), "root".into()],
                vec!["e".into(), parent.into(), "".into(), "reply".into()],
            ],
        )
    }

    #[test]
    fn empty_open_yields_empty_snapshot() {
        let proj = ModularTimelineProjection::new(spec());
        let snap = proj.snapshot();
        assert!(snap.blocks.is_empty());
        assert!(snap.cards.is_empty());
    }

    #[test]
    fn root_plus_reply_collapses_into_one_module() {
        let proj = ModularTimelineProjection::new(spec());
        proj.on_kernel_event(&note("R", 1, vec![]));
        proj.on_kernel_event(&reply_to("C", 2, "R", "R"));
        let snap = proj.snapshot();
        assert_eq!(snap.blocks.len(), 1);
        match &snap.blocks[0] {
            TimelineBlock::Module { events, .. } => {
                assert_eq!(events, &vec!["R".to_string(), "C".to_string()]);
            }
            other => panic!("expected Module, got {other:?}"),
        }
        assert_eq!(snap.cards.len(), 2);
    }

    #[test]
    fn standalone_event_becomes_standalone_block() {
        let proj = ModularTimelineProjection::new(spec());
        proj.on_kernel_event(&note("S", 1, vec![]));
        let snap = proj.snapshot();
        assert_eq!(snap.blocks.len(), 1);
        assert!(matches!(snap.blocks[0], TimelineBlock::Standalone(_)));
    }

    #[test]
    fn cards_include_content_tree_wire_for_mentions() {
        const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
        let mention = format!("nostr:{}", encode_npub(PK).expect("fixture npub encodes"));
        let proj = ModularTimelineProjection::new(spec());
        proj.on_kernel_event(&note_with_content(
            "S",
            1,
            vec![],
            &format!("hello {mention} #nostr"),
        ));

        let snap = proj.snapshot();
        let card = snap.cards.iter().find(|c| c.id == "S").expect("card exists");
        assert!(card.content_tree.nodes.iter().any(|node| {
            matches!(
                node,
                WireNode::Mention { uri }
                    if uri.kind == WireNostrUriKind::Profile && uri.primary_id == PK
            )
        }));
    }

    #[test]
    fn observer_trait_object_drives_grouper() {
        let proj: Arc<dyn KernelEventObserver> = Arc::new(ModularTimelineProjection::new(spec()));
        proj.on_kernel_event(&note("X", 1, vec![]));
    }
}
