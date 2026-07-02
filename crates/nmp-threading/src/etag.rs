//! Kind-blind resolver for NIP-10-style `e` tag reply/root grammar.
//!
//! The resolver intentionally ignores event kind. It interprets the same
//! marked/positional `e` tag grammar for any caller-supplied event scope, so a
//! NIP-29 group timeline, a note feed, or any other scoped event stream can
//! reuse one threading read model without app-side tag parsing.

use nmp_core::substrate::KernelEvent;
use nmp_core::tags::parse_nip10;

use crate::{ParentResolver, ThreadPointer};

/// [`ParentResolver`] over raw `e` tags, independent of event kind.
#[derive(Clone, Copy, Debug, Default)]
pub struct EtagThreadResolver;

impl ParentResolver for EtagThreadResolver {
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
        refs.mentioned_pubkeys.into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(tags: Vec<Vec<&str>>) -> KernelEvent {
        KernelEvent {
            id: "child".to_string(),
            author: "author".to_string(),
            kind: 1,
            created_at: 100,
            tags: tags
                .into_iter()
                .map(|t| t.into_iter().map(str::to_string).collect())
                .collect(),
            content: String::new(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn resolves_marked_root_and_reply() {
        let e = event(vec![
            vec!["e", &"1".repeat(64), "", "root"],
            vec!["e", &"2".repeat(64), "", "reply"],
            vec!["p", &"3".repeat(64)],
        ]);
        let resolver = EtagThreadResolver;
        assert_eq!(
            resolver.root(&e).unwrap().event_id(),
            Some("1".repeat(64).as_str())
        );
        assert_eq!(
            resolver.parent(&e).unwrap().event_id(),
            Some("2".repeat(64).as_str())
        );
        assert_eq!(resolver.parent_author(&e), Some("3".repeat(64)));
    }

    #[test]
    fn root_only_event_has_no_parent() {
        let e = event(vec![]);
        let resolver = EtagThreadResolver;
        assert!(resolver.root(&e).is_none());
        assert!(resolver.parent(&e).is_none());
    }
}
