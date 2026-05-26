//! `Nip10ModularTimelineView` — modular threaded timeline over NIP-10
//! short-note reply graphs. Wraps the agnostic [`nmp_threading::Grouper`]
//! with a NIP-10 [`ParentResolver`] driven by
//! [`nmp_core::tags::parse_nip10`].
//!
//! Replaces the doctrine-violating `MetaTimelineViewModule` mention in
//! `nmp-core/src/planner/interest.rs` — that work now lives here in a
//! sibling protocol crate per ADR-0009.

use std::collections::BTreeSet;

use nmp_core::substrate::{EventId, KernelEvent, ViewContext, ViewDependencies};
use nmp_core::tags::parse_nip10;
use nmp_nip18::{try_from_kernel_event as try_from_repost_event, KIND_REPOST};
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
        if event.kind == KIND_REPOST {
            return None;
        }
        let refs = parse_nip10(&event.tags);
        refs.reply.map(|r| ThreadPointer::Event {
            id: r.id,
            relay: r.relay,
            kind: None,
        })
    }

    fn root(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        if event.kind == KIND_REPOST {
            return None;
        }
        let refs = parse_nip10(&event.tags);
        refs.root.map(|r| ThreadPointer::Event {
            id: r.id,
            relay: r.relay,
            kind: None,
        })
    }

    fn parent_author(&self, event: &KernelEvent) -> Option<String> {
        if event.kind == KIND_REPOST {
            return None;
        }
        let refs = parse_nip10(&event.tags);
        // Best-effort: NIP-10 says the participants' p-tags accompany the
        // reply, but there's no positional guarantee that the first p-tag
        // is the parent's author. Return the first p-tag — callers treat
        // this as a hint, not authoritative.
        refs.mentioned_pubkeys.into_iter().next()
    }

    fn supersedes(&self, event: &KernelEvent) -> Option<String> {
        // NIP-18: a kind:6 repost bumps the original note in the feed. The
        // grouper evicts the target's standalone block on the way in and
        // suppresses it if it arrives later — the note renders once, at the
        // repost's position, attributed to the repost author.
        if event.kind != KIND_REPOST {
            return None;
        }
        try_from_repost_event(event).and_then(|record| record.target_event_id)
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

/// Modular timeline over NIP-10 short-note reply graphs, exported as a public
/// type for per-app composition. Reposts can be admitted by the view spec, but
/// their `e` tags are target references rather than reply edges, so they stay
/// standalone in the grouped timeline.
///
/// Once an `impl ViewModule`; now a plain type whose inherent methods are
/// reached via static dispatch — `ModularTimelineProjection` (the live
/// `KernelEventObserver` consumer) drives `open` / `snapshot` /
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
        spec: &ModularTimelineSpec,
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

    #[must_use]
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

    #[must_use]
    pub fn on_event_removed(
        _c: &ViewContext,
        s: &mut ModularTimelineState,
        id: &EventId,
    ) -> Option<ModularTimelineDelta> {
        s.grouper.on_remove(id).map(Into::into)
    }

    #[must_use]
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
    pub fn snapshot(_c: &ViewContext, state: &ModularTimelineState) -> ModularTimelinePayload {
        ModularTimelinePayload {
            blocks: state.grouper.blocks().to_vec(),
        }
    }
}

#[cfg(test)]
#[path = "meta_timeline/tests.rs"]
mod tests;
