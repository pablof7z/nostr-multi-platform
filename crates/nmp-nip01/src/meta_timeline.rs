//! `Nip10ModularTimelineView` — modular threaded timeline over NIP-10
//! kind:1 events. Wraps the agnostic [`nmp_threading::Grouper`] with a
//! NIP-10 [`ParentResolver`] driven by [`nmp_core::tags::parse_nip10`].
//!
//! Replaces the doctrine-violating `MetaTimelineViewModule` mention in
//! `nmp-core/src/planner/interest.rs` — that work now lives here in a
//! sibling protocol crate per ADR-0009.

use std::collections::BTreeSet;

use nmp_core::substrate::{EventId, KernelEvent, ViewContext, ViewDependencies};
use nmp_core::tags::parse_nip10;
use nmp_threading::{
    GroupDelta, Grouper, ModulePolicy, ParentResolver, ThreadPointer, TimelineBlock,
};
use serde::{Deserialize, Serialize};

use crate::kinds::KIND_SHORT_NOTE;

/// Pubkey alias mirroring the planner.
pub type Pubkey = String;

/// `ParentResolver` over NIP-10 markers. The grouper never sees kind:1
/// directly — it only sees this resolver's `ThreadPointer` answers.
pub struct Nip10Resolver;

impl ParentResolver for Nip10Resolver {
    fn parent(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        let refs = parse_nip10(&event.tags);
        refs.reply.map(|r| ThreadPointer::Event {
            id: r.id,
            relay: r.relay,
            kind: None,
        })
    }

    fn root(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        let refs = parse_nip10(&event.tags);
        refs.root.map(|r| ThreadPointer::Event {
            id: r.id,
            relay: r.relay,
            kind: None,
        })
    }

    fn parent_author(&self, event: &KernelEvent) -> Option<String> {
        let refs = parse_nip10(&event.tags);
        // Best-effort: NIP-10 says the participants' p-tags accompany the
        // reply, but there's no positional guarantee that the first p-tag
        // is the parent's author. Return the first p-tag — callers treat
        // this as a hint, not authoritative.
        refs.mentioned_pubkeys.into_iter().next()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModularTimelineSpec {
    /// Viewer pubkey — included for future mute / personalization keys.
    pub viewer: Pubkey,
    /// Event kinds to admit. Defaults to `[1]` when empty.
    #[serde(default)]
    pub kinds: Vec<u32>,
    /// Author filter. `None` accepts any author the planner surfaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<Pubkey>>,
    /// Grouping policy.
    #[serde(default)]
    pub policy: ModulePolicy,
}

impl ModularTimelineSpec {
    #[must_use] 
    pub fn effective_kinds(&self) -> Vec<u32> {
        if self.kinds.is_empty() {
            vec![KIND_SHORT_NOTE]
        } else {
            self.kinds.clone()
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModularTimelinePayload {
    pub blocks: Vec<TimelineBlock>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum ModularTimelineDelta {
    BlockInserted(usize),
    BlockReplaced(usize),
    BlockRemoved(usize),
}

impl From<GroupDelta> for ModularTimelineDelta {
    fn from(d: GroupDelta) -> Self {
        match d {
            GroupDelta::BlockInserted(i) => Self::BlockInserted(i),
            GroupDelta::BlockReplaced(i) => Self::BlockReplaced(i),
            GroupDelta::BlockRemoved(i) => Self::BlockRemoved(i),
        }
    }
}

pub struct ModularTimelineState {
    grouper: Grouper<Nip10Resolver>,
    accepted_kinds: BTreeSet<u32>,
    accepted_authors: Option<BTreeSet<Pubkey>>,
}

impl ModularTimelineState {
    fn admits(&self, event: &KernelEvent) -> bool {
        if !self.accepted_kinds.contains(&event.kind) {
            return false;
        }
        if let Some(auth) = &self.accepted_authors {
            if !auth.contains(&event.author) {
                return false;
            }
        }
        true
    }
}

/// Modular timeline over NIP-10 kind:1 events, exported as a public type for
/// per-app composition. Once an `impl ViewModule`; now a plain type whose
/// inherent methods are reached via static dispatch — `ModularTimelineProjection`
/// (the live `KernelEventObserver` consumer) drives `open` / `snapshot` /
/// `on_event_inserted` directly.
pub struct Nip10ModularTimelineView;

impl Nip10ModularTimelineView {
    pub const NAMESPACE: &'static str = "nmp.nip01.modular_timeline";

    #[must_use] 
    pub fn key(spec: &ModularTimelineSpec) -> String {
        // One open view per (viewer, author-set, kinds) tuple.
        let mut k = format!("{}|", spec.viewer);
        let mut kinds = spec.effective_kinds();
        kinds.sort_unstable();
        for kind in &kinds {
            k.push_str(&kind.to_string());
            k.push(',');
        }
        k.push('|');
        if let Some(authors) = &spec.authors {
            let mut sorted = authors.clone();
            sorted.sort();
            for a in sorted {
                k.push_str(&a);
                k.push(',');
            }
        }
        k
    }

    #[must_use] 
    pub fn dependencies(spec: &ModularTimelineSpec) -> ViewDependencies {
        ViewDependencies {
            kinds: spec.effective_kinds(),
            authors: spec.authors.clone().unwrap_or_default(),
            ..Default::default()
        }
    }

    #[must_use] 
    pub fn open(
        _ctx: &ViewContext,
        spec: ModularTimelineSpec,
    ) -> (ModularTimelineState, ModularTimelinePayload) {
        let accepted_kinds: BTreeSet<u32> = spec.effective_kinds().into_iter().collect();
        let accepted_authors = spec
            .authors
            .as_ref()
            .map(|v| v.iter().cloned().collect::<BTreeSet<_>>());
        let state = ModularTimelineState {
            grouper: Grouper::new(Nip10Resolver, spec.policy.clone()),
            accepted_kinds,
            accepted_authors,
        };
        let payload = ModularTimelinePayload { blocks: Vec::new() };
        (state, payload)
    }

    pub fn on_event_inserted(
        _c: &ViewContext,
        s: &mut ModularTimelineState,
        e: &KernelEvent,
    ) -> Option<ModularTimelineDelta> {
        if !s.admits(e) {
            return None;
        }
        s.grouper.on_insert(e).map(Into::into)
    }

    pub fn on_event_removed(
        _c: &ViewContext,
        s: &mut ModularTimelineState,
        id: &EventId,
    ) -> Option<ModularTimelineDelta> {
        s.grouper.on_remove(id).map(Into::into)
    }

    pub fn on_event_replaced(
        _c: &ViewContext,
        s: &mut ModularTimelineState,
        old: &EventId,
        e: &KernelEvent,
    ) -> Option<ModularTimelineDelta> {
        if !s.admits(e) {
            return s.grouper.on_remove(old).map(Into::into);
        }
        s.grouper.on_replace(old, e).map(Into::into)
    }

    #[must_use] 
    pub fn snapshot(
        _c: &ViewContext,
        state: &ModularTimelineState,
    ) -> ModularTimelinePayload {
        ModularTimelinePayload {
            blocks: state.grouper.blocks().to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ViewContext {
        ViewContext::default()
    }

    fn note(id: &str, ts: u64, tags: Vec<Vec<String>>) -> KernelEvent {
        KernelEvent {
            id: id.into(),
            author: "auth".into(),
            kind: 1,
            created_at: ts,
            tags,
            content: id.into(),
        }
    }

    fn marked(id: &str, ts: u64, root: &str, parent: &str) -> KernelEvent {
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
    fn empty_open_yields_empty_payload() {
        let spec = ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            authors: None,
            policy: ModulePolicy::default(),
        };
        let (_state, payload) = Nip10ModularTimelineView::open(&ctx(), spec);
        assert!(payload.blocks.is_empty());
    }

    #[test]
    fn dependencies_default_to_kind_1() {
        let spec = ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            authors: None,
            policy: ModulePolicy::default(),
        };
        let deps = Nip10ModularTimelineView::dependencies(&spec);
        assert_eq!(deps.kinds, vec![1]);
        assert!(deps.authors.is_empty());
    }

    #[test]
    fn root_plus_reply_form_single_module() {
        let spec = ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            authors: None,
            policy: ModulePolicy::default(),
        };
        let (mut s, _) = Nip10ModularTimelineView::open(&ctx(), spec);
        let root = note("R", 1, vec![]);
        let reply = marked("C", 2, "R", "R");
        Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &root);
        Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &reply);
        let snap = Nip10ModularTimelineView::snapshot(&ctx(), &s);
        assert_eq!(snap.blocks.len(), 1);
        match &snap.blocks[0] {
            TimelineBlock::Module { events, .. } => {
                assert_eq!(events, &vec!["R".to_string(), "C".to_string()]);
            }
            other => panic!("expected Module, got {other:?}"),
        }
    }

    #[test]
    fn rejects_wrong_kind() {
        let spec = ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            authors: None,
            policy: ModulePolicy::default(),
        };
        let (mut s, _) = Nip10ModularTimelineView::open(&ctx(), spec);
        let mut e = note("Z", 1, vec![]);
        e.kind = 30023;
        assert!(Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &e).is_none());
        assert!(Nip10ModularTimelineView::snapshot(&ctx(), &s).blocks.is_empty());
    }

    #[test]
    fn author_filter_excludes_others() {
        let spec = ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            authors: Some(vec!["alice".into()]),
            policy: ModulePolicy::default(),
        };
        let (mut s, _) = Nip10ModularTimelineView::open(&ctx(), spec);
        let mut a = note("A", 1, vec![]);
        a.author = "alice".into();
        let mut b = note("B", 2, vec![]);
        b.author = "bob".into();
        Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &a);
        Nip10ModularTimelineView::on_event_inserted(&ctx(), &mut s, &b);
        let snap = Nip10ModularTimelineView::snapshot(&ctx(), &s);
        assert_eq!(snap.blocks.len(), 1);
    }

    // ── ModularTimelineSpec::effective_kinds ────────────────────────────────

    #[test]
    fn effective_kinds_defaults_to_kind_1_when_empty() {
        let spec = ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![],
            authors: None,
            policy: ModulePolicy::default(),
        };
        assert_eq!(spec.effective_kinds(), vec![KIND_SHORT_NOTE]);
    }

    #[test]
    fn effective_kinds_passes_explicit_kinds_through_verbatim() {
        let spec = ModularTimelineSpec {
            viewer: "me".into(),
            kinds: vec![1, 6, 16],
            authors: None,
            policy: ModulePolicy::default(),
        };
        // No defaulting, no sorting at this layer — `key()` owns ordering.
        assert_eq!(spec.effective_kinds(), vec![1, 6, 16]);
    }

    // ── Nip10ModularTimelineView::key determinism ───────────────────────────

    fn spec_with(kinds: Vec<u32>, authors: Option<Vec<&str>>) -> ModularTimelineSpec {
        ModularTimelineSpec {
            viewer: "viewer".into(),
            kinds,
            authors: authors.map(|v| v.into_iter().map(String::from).collect()),
            policy: ModulePolicy::default(),
        }
    }

    #[test]
    fn key_is_order_independent_for_kinds() {
        // Two specs with the same kind *set* in different input order must key
        // identically — otherwise the same logical view opens twice.
        let a = Nip10ModularTimelineView::key(&spec_with(vec![16, 1, 6], None));
        let b = Nip10ModularTimelineView::key(&spec_with(vec![1, 6, 16], None));
        assert_eq!(a, b);
    }

    #[test]
    fn key_is_order_independent_for_authors() {
        let a = Nip10ModularTimelineView::key(&spec_with(vec![], Some(vec!["carol", "alice", "bob"])));
        let b = Nip10ModularTimelineView::key(&spec_with(vec![], Some(vec!["alice", "bob", "carol"])));
        assert_eq!(a, b);
    }

    #[test]
    fn key_distinguishes_different_viewers_and_author_sets() {
        let one = Nip10ModularTimelineView::key(&spec_with(vec![], Some(vec!["alice"])));
        let two = Nip10ModularTimelineView::key(&spec_with(vec![], Some(vec!["bob"])));
        assert_ne!(one, two, "different author sets must not collide");

        // `None` authors and `Some(empty)` authors are distinguishable from a
        // populated set.
        let no_filter = Nip10ModularTimelineView::key(&spec_with(vec![], None));
        assert_ne!(no_filter, one);
    }

    #[test]
    fn key_empty_kinds_resolves_to_the_kind_1_default() {
        // `key()` uses `effective_kinds()`, so an empty-kinds spec and an
        // explicit `[1]` spec must produce the same key.
        let defaulted = Nip10ModularTimelineView::key(&spec_with(vec![], None));
        let explicit = Nip10ModularTimelineView::key(&spec_with(vec![1], None));
        assert_eq!(defaulted, explicit);
    }

    // ── From<GroupDelta> for ModularTimelineDelta ───────────────────────────

    #[test]
    fn group_delta_converts_to_modular_timeline_delta_for_every_arm() {
        assert_eq!(
            ModularTimelineDelta::from(GroupDelta::BlockInserted(3)),
            ModularTimelineDelta::BlockInserted(3)
        );
        assert_eq!(
            ModularTimelineDelta::from(GroupDelta::BlockReplaced(7)),
            ModularTimelineDelta::BlockReplaced(7)
        );
        assert_eq!(
            ModularTimelineDelta::from(GroupDelta::BlockRemoved(0)),
            ModularTimelineDelta::BlockRemoved(0)
        );
    }

    // ── Nip10Resolver ───────────────────────────────────────────────────────

    #[test]
    fn resolver_extracts_reply_and_root_pointers() {
        let reply = marked("C", 2, "ROOT", "PARENT");
        match Nip10Resolver.parent(&reply) {
            Some(ThreadPointer::Event { id, kind, .. }) => {
                assert_eq!(id, "PARENT");
                assert_eq!(kind, None);
            }
            other => panic!("expected an Event parent pointer, got {other:?}"),
        }
        match Nip10Resolver.root(&reply) {
            Some(ThreadPointer::Event { id, .. }) => assert_eq!(id, "ROOT"),
            other => panic!("expected an Event root pointer, got {other:?}"),
        }
    }

    #[test]
    fn resolver_returns_none_for_a_thread_root() {
        // A root note carries no NIP-10 markers — both parent() and root()
        // resolve to None so the grouper treats it as a module head.
        let root = note("R", 1, vec![]);
        assert!(Nip10Resolver.parent(&root).is_none());
        assert!(Nip10Resolver.root(&root).is_none());
        assert!(Nip10Resolver.parent_author(&root).is_none());
    }

    #[test]
    fn resolver_parent_author_returns_first_p_tag_as_a_hint() {
        // NIP-10 gives no positional guarantee for p-tags; the resolver
        // surfaces the first p-tag as a best-effort hint. Pin that behaviour.
        let mut e = marked("C", 2, "ROOT", "PARENT");
        e.tags.push(vec!["p".into(), "first-pubkey".into()]);
        e.tags.push(vec!["p".into(), "second-pubkey".into()]);
        assert_eq!(
            Nip10Resolver.parent_author(&e),
            Some("first-pubkey".to_string())
        );
    }

    #[test]
    fn resolver_carries_relay_hint_from_marked_e_tag() {
        // A marked `e` tag with a relay column must surface that relay on the
        // resolved ThreadPointer.
        let e = note(
            "C",
            2,
            vec![vec![
                "e".into(),
                "PARENT".into(),
                "wss://relay.example".into(),
                "reply".into(),
            ]],
        );
        match Nip10Resolver.parent(&e) {
            Some(ThreadPointer::Event { id, relay, .. }) => {
                assert_eq!(id, "PARENT");
                assert_eq!(relay.as_deref(), Some("wss://relay.example"));
            }
            other => panic!("expected an Event pointer with relay, got {other:?}"),
        }
    }
}
