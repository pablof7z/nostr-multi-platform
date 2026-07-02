//! Shared fixtures for the grouper integration tests: a fake resolver that
//! decodes convention-agnostic tags (`e_parent`, `e_root`, `a_root`,
//! `i_root`, `e_supersedes`), event builders on top of it, and the
//! `block_ids` layout helper used across every behavior-area submodule.

use nmp_core::substrate::KernelEvent;
use nmp_threading::{Grouper, ModulePolicy, ParentResolver, ThreadPointer, TimelineBlock};

pub(crate) struct FakeResolver;

pub(crate) fn tag(key: &str, val: &str) -> Vec<String> {
    vec![key.into(), val.into()]
}

pub(crate) fn event_with_tags(id: &str, created_at: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: "auth".into(),
        kind: 1,
        created_at,
        tags,
        content: id.into(),
        relay_provenance: Vec::new(),
    }
}

pub(crate) fn parent_tags(parent: Option<&str>) -> Vec<Vec<String>> {
    let mut tags = Vec::new();
    if let Some(p) = parent {
        tags.push(tag("e_parent", p));
    }
    tags
}

pub(crate) fn ev(
    id: &str,
    created_at: u64,
    parent: Option<&str>,
    root: Option<&str>,
) -> KernelEvent {
    let mut tags = parent_tags(parent);
    if let Some(r) = root {
        tags.push(tag("e_root", r));
    }
    event_with_tags(id, created_at, tags)
}

pub(crate) fn ev_addr_root(
    id: &str,
    created_at: u64,
    parent: Option<&str>,
    coord: &str,
) -> KernelEvent {
    let mut tags = parent_tags(parent);
    tags.push(tag("a_root", coord));
    event_with_tags(id, created_at, tags)
}

pub(crate) fn ev_uri_root(
    id: &str,
    created_at: u64,
    parent: Option<&str>,
    uri: &str,
) -> KernelEvent {
    let mut tags = parent_tags(parent);
    tags.push(tag("i_root", uri));
    event_with_tags(id, created_at, tags)
}

impl ParentResolver for FakeResolver {
    fn parent(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        event.tags.iter().find_map(|t| match (t.first(), t.get(1)) {
            (Some(k), Some(v)) if k == "e_parent" => Some(ThreadPointer::Event {
                id: v.clone(),
                relay: None,
                kind: None,
            }),
            _ => None,
        })
    }
    fn root(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        event.tags.iter().find_map(|t| match (t.first(), t.get(1)) {
            (Some(k), Some(v)) if k == "e_root" => Some(ThreadPointer::Event {
                id: v.clone(),
                relay: None,
                kind: None,
            }),
            (Some(k), Some(v)) if k == "a_root" => Some(ThreadPointer::Address {
                coord: v.clone(),
                relay: None,
                kind: None,
            }),
            (Some(k), Some(v)) if k == "i_root" => Some(ThreadPointer::External { uri: v.clone() }),
            _ => None,
        })
    }
    fn parent_author(&self, _event: &KernelEvent) -> Option<String> {
        None
    }
    fn supersedes(&self, event: &KernelEvent) -> Option<String> {
        event.tags.iter().find_map(|t| match (t.first(), t.get(1)) {
            (Some(k), Some(v)) if k == "e_supersedes" => Some(v.clone()),
            _ => None,
        })
    }
}

pub(crate) fn ev_supersedes(id: &str, created_at: u64, target: &str) -> KernelEvent {
    event_with_tags(id, created_at, vec![tag("e_supersedes", target)])
}

pub(crate) fn fresh() -> Grouper<FakeResolver> {
    Grouper::new(FakeResolver, ModulePolicy::default())
}

pub(crate) fn block_ids(b: &TimelineBlock) -> Vec<&str> {
    match b {
        TimelineBlock::Standalone { id, .. } => vec![id.as_str()],
        TimelineBlock::Module { events, .. } => events.iter().map(|s| s.as_str()).collect(),
    }
}
