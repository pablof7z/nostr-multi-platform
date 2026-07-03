//! Kind-blind resolver for marked/positional `e` tag reply/root grammar.
//!
//! The resolver intentionally ignores event kind. It interprets the same
//! marked/positional `e` tag shape for any caller-supplied event scope, so
//! NIP-29 group timelines, note feeds, or other scoped event streams can reuse
//! one threading read model without app-side tag parsing.

use nmp_core::substrate::KernelEvent;

use crate::{ParentResolver, ThreadPointer};

/// [`ParentResolver`] over raw `e` tags, independent of event kind.
#[derive(Clone, Copy, Debug, Default)]
pub struct EtagThreadResolver;

impl ParentResolver for EtagThreadResolver {
    fn parent(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        parse_e_refs(&event.tags)
            .reply
            .map(|r| ThreadPointer::Event {
                id: r.id,
                relay: r.relay,
                kind: None,
            })
    }

    fn root(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        parse_e_refs(&event.tags)
            .root
            .map(|r| ThreadPointer::Event {
                id: r.id,
                relay: r.relay,
                kind: None,
            })
    }

    fn parent_author(&self, event: &KernelEvent) -> Option<String> {
        event
            .tags
            .iter()
            .find(|tag| tag.first().map(String::as_str) == Some("p"))
            .and_then(|tag| tag.get(1).cloned())
    }
}

#[derive(Clone)]
struct ERef {
    id: String,
    relay: Option<String>,
    marker: Option<String>,
}

#[derive(Default)]
struct ERefs {
    root: Option<ERef>,
    reply: Option<ERef>,
}

fn parse_e_refs(tags: &[Vec<String>]) -> ERefs {
    let e_tags: Vec<&Vec<String>> = tags
        .iter()
        .filter(|tag| tag.first().map(String::as_str) == Some("e"))
        .collect();
    let has_marker = e_tags.iter().any(|tag| {
        matches!(
            tag.get(3).map(String::as_str),
            Some("root" | "reply" | "mention")
        )
    });

    if has_marker {
        let mut refs = ERefs::default();
        for tag in e_tags {
            let Some(eref) = e_ref(tag) else { continue };
            match eref.marker.as_deref() {
                Some("root") if refs.root.is_none() => refs.root = Some(eref),
                Some("reply") if refs.reply.is_none() => refs.reply = Some(eref),
                _ => {}
            }
        }
        if refs.reply.is_none() {
            refs.reply = refs.root.clone();
        }
        return refs;
    }

    let resolved: Vec<ERef> = e_tags.into_iter().filter_map(|tag| e_ref(tag)).collect();
    match resolved.len() {
        0 => ERefs::default(),
        1 => ERefs {
            root: Some(resolved[0].clone()),
            reply: Some(resolved[0].clone()),
        },
        n => ERefs {
            root: Some(resolved[0].clone()),
            reply: Some(resolved[n - 1].clone()),
        },
    }
}

fn e_ref(tag: &[String]) -> Option<ERef> {
    let id = tag.get(1)?.clone();
    if id.is_empty() {
        return None;
    }
    Some(ERef {
        id,
        relay: tag.get(2).filter(|s| !s.is_empty()).cloned(),
        marker: tag.get(3).filter(|s| !s.is_empty()).cloned(),
    })
}
