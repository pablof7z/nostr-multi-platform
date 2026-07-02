//! Kind-blind resolver for NIP-10-style `e` tag reply/root grammar.
//!
//! The resolver intentionally ignores event kind. It interprets the same
//! marked/positional `e` tag grammar for any caller-supplied event scope, so
//! NIP-29 group timelines, note feeds, or other scoped event streams can reuse
//! one threading read model without app-side tag parsing.

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
