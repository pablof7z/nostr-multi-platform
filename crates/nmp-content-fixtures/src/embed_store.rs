//! Relay-free pre-resolved embed store.
//!
//! This is the offline analogue of what `EmbedClaimRegistry` would serve at
//! runtime: a map from `nostr:` URI → **resolution facts** (the resolved
//! target + its rendered body). It deliberately does NOT enforce the
//! PD-015 depth budget or the `visited`-set cycle guard — those are
//! render-time concerns that travel with the renderer's traversal, not
//! with the resolved data (a flat URI map has one slot per URI, but a
//! cyclic URI appears at multiple depths). The Swift walker (STAGE 3)
//! mirrors `RenderContext::should_collapse` at walk time. The only
//! context-independent collapse facts the bundle carries are `"dangling"`
//! (URI absent from the store) and `"unsupported"` (kind has no NMP view).

use std::collections::BTreeMap;

use nmp_content::{tokenize_with_kind, RenderMode};
use nmp_core::substrate::SignedEvent;

use crate::dto::{
    ArticleHeaderDto, ContentTreeDto, EmbedEntry, ListDto, SignedEventJson,
};
use crate::project::project_tree;

/// A target registered in the offline store, keyed by its `nostr:` URI.
pub enum Target {
    /// kind:0 profile metadata.
    Profile {
        /// Display name, if any.
        name: Option<String>,
        /// Picture URL, if any.
        picture: Option<String>,
    },
    /// A resolvable event (note / article / list / unknown kind).
    Event(SignedEvent),
    /// An article preview (kind:30023) — header + the underlying event.
    Article {
        /// The signed kind:30023 event.
        event: SignedEvent,
        /// Projected header.
        header: ArticleHeaderDto,
    },
    /// A NIP-51 list (kind:30000 / 30003 / 10002).
    List {
        /// The signed list event.
        event: SignedEvent,
        /// Projected list rows.
        list: ListDto,
    },
}

/// Builder for a scenario's relay-free embed store.
#[derive(Default)]
pub struct EmbedStore {
    targets: BTreeMap<String, Target>,
}

fn to_json(ev: &SignedEvent) -> SignedEventJson {
    SignedEventJson {
        id: ev.id.clone(),
        pubkey: ev.unsigned.pubkey.clone(),
        created_at: ev.unsigned.created_at,
        kind: ev.unsigned.kind,
        tags: ev.unsigned.tags.clone(),
        content: ev.unsigned.content.clone(),
        sig: ev.sig.clone(),
    }
}

impl EmbedStore {
    /// Register a target under its `nostr:` URI.
    pub fn add(&mut self, uri: impl Into<String>, target: Target) {
        self.targets.insert(uri.into(), target);
    }

    fn render_event_body(ev: &SignedEvent) -> ContentTreeDto {
        let tree = tokenize_with_kind(
            &ev.unsigned.content,
            &ev.unsigned.tags,
            RenderMode::Auto,
            ev.unsigned.kind,
        );
        project_tree(&tree)
    }

    /// Resolve one URI against the store into context-independent facts.
    /// No depth/cycle guard here — that is the renderer's job (STAGE 3).
    fn resolve_one(&self, uri: &str) -> EmbedEntry {
        let Some(target) = self.targets.get(uri) else {
            // Dangling: target never added to the relay-free store. D1
            // best-effort — collapsed stub, never a spinner. This IS a
            // context-independent fact (a property of the store).
            return EmbedEntry {
                resolved_kind: 0,
                profile_name: None,
                profile_picture: None,
                event: None,
                rendered: None,
                collapsed: true,
                collapse_reason: Some("dangling".to_string()),
                article: None,
                list: None,
            };
        };

        match target {
            Target::Profile { name, picture } => EmbedEntry {
                resolved_kind: 0,
                profile_name: name.clone(),
                profile_picture: picture.clone(),
                event: None,
                rendered: None,
                collapsed: false,
                collapse_reason: None,
                article: None,
                list: None,
            },
            Target::Event(ev) => self.event_entry(ev, None, None),
            Target::Article { event, header } => {
                self.event_entry(event, Some(header.clone()), None)
            }
            Target::List { event, list } => {
                self.event_entry(event, None, Some(list.clone()))
            }
        }
    }

    fn event_entry(
        &self,
        ev: &SignedEvent,
        article: Option<ArticleHeaderDto>,
        list: Option<ListDto>,
    ) -> EmbedEntry {
        let kind = ev.unsigned.kind;

        // Unknown/unsupported kind → graceful neutral card (S-E02). This
        // is a context-independent fact (a property of the event kind).
        let known = kind == 1
            || kind == 30023
            || kind == 30000
            || kind == 30003
            || kind == 10002;
        if !known {
            return EmbedEntry {
                resolved_kind: kind,
                profile_name: None,
                profile_picture: None,
                event: Some(to_json(ev)),
                rendered: None,
                collapsed: true,
                collapse_reason: Some("unsupported".to_string()),
                article,
                list,
            };
        }

        let rendered = Self::render_event_body(ev);
        EmbedEntry {
            resolved_kind: kind,
            profile_name: None,
            profile_picture: None,
            event: Some(to_json(ev)),
            rendered: Some(rendered),
            collapsed: false,
            collapse_reason: None,
            article,
            list,
        }
    }

    /// Resolve **every** URI transitively reachable from the primary tree
    /// (including through resolved embed bodies). Each URI is resolved
    /// once, unconditionally and fully — the map is a set of resolution
    /// facts. A cyclic reference terminates the transitive walk via the
    /// `out.contains_key` visited check (resolution-dedup, NOT a render
    /// cycle guard): the renderer re-derives PD-015 collapse at walk time.
    pub fn resolve_all(
        &self,
        root: &ContentTreeDto,
    ) -> BTreeMap<String, EmbedEntry> {
        let mut out = BTreeMap::new();
        self.walk(root, &mut out);
        out
    }

    fn walk(
        &self,
        tree: &ContentTreeDto,
        out: &mut BTreeMap<String, EmbedEntry>,
    ) {
        let mut uris = Vec::new();
        for seg in &tree.segments {
            collect_uris(seg, &mut uris);
        }
        for uri in uris {
            if out.contains_key(&uri) {
                continue;
            }
            let entry = self.resolve_one(&uri);
            let child = entry.rendered.clone();
            out.insert(uri, entry);
            if let Some(child) = child {
                self.walk(&child, out);
            }
        }
    }
}

/// Collect every embed-bearing URI in a segment, descending into Markdown
/// block + inline nodes (article bodies render in Markdown mode, so a
/// `nostr:` reference can be nested inside a Paragraph/List/BlockQuote).
fn collect_uris(seg: &crate::dto::SegmentDto, out: &mut Vec<String>) {
    use crate::dto::SegmentDto as S;
    match seg {
        S::Mention { uri, .. } | S::EventRef { uri, .. } => {
            out.push(uri.clone())
        }
        S::MarkdownBlock { node } => collect_node_uris(node, out),
        _ => {}
    }
}

fn collect_node_uris(
    node: &crate::dto::MarkdownNodeDto,
    out: &mut Vec<String>,
) {
    use crate::dto::MarkdownNodeDto as N;
    match node {
        N::Heading { inlines, .. } | N::Paragraph { inlines } => {
            for i in inlines {
                collect_inline_uris(i, out);
            }
        }
        N::BlockQuote { blocks } => {
            for b in blocks {
                collect_node_uris(b, out);
            }
        }
        N::List { items, .. } => {
            for item in items {
                for b in item {
                    collect_node_uris(b, out);
                }
            }
        }
        N::CodeBlock { .. } | N::Rule => {}
    }
}

fn collect_inline_uris(
    inline: &crate::dto::MarkdownInlineDto,
    out: &mut Vec<String>,
) {
    use crate::dto::MarkdownInlineDto as I;
    match inline {
        I::Inline { segment } => collect_uris(segment, out),
        I::Emphasis { children }
        | I::Strong { children }
        | I::Link { label: children, .. } => {
            for c in children {
                collect_inline_uris(c, out);
            }
        }
        I::Code { .. }
        | I::Image { .. }
        | I::SoftBreak
        | I::HardBreak => {}
    }
}
