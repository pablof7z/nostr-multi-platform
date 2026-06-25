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

use nmp_content::{
    resolve_embed_projection, tokenize_with_kind, EmbedKindProjection, RenderContext, RenderMode,
};
use nmp_core::kinds::is_addressable;
use nmp_core::substrate::KernelEvent;
use nmp_signer_iface::SignedEvent;

use crate::dto::{ContentTreeDto, EmbedEntry, SignedEventJson};
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
    /// A resolvable event (note / article / list / highlight / unknown kind).
    /// The typed per-kind projection is derived from the event's own tags +
    /// content by the canonical `resolve_embed_projection` resolver — there is
    /// no longer a hand-rolled article/list projection at the fixture layer.
    Event(SignedEvent),
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

fn to_kernel_event(ev: &SignedEvent) -> KernelEvent {
    KernelEvent {
        id: ev.id.clone(),
        author: ev.unsigned.pubkey.clone(),
        kind: ev.unsigned.kind,
        created_at: ev.unsigned.created_at,
        tags: ev.unsigned.tags.clone(),
        content: ev.unsigned.content.clone(),
        relay_provenance: Vec::new(),
    }
}

/// Derive the canonical typed per-kind projection for a resolved event using
/// the single workspace dispatch point (`resolve_embed_projection`). The
/// fixture store carries this so native registries dispatch on the same shape
/// the runtime resolver emits — no parallel article/list projection here.
fn kind_projection(ev: &SignedEvent) -> EmbedKindProjection {
    resolve_embed_projection(&to_kernel_event(ev), &RenderContext::new())
}

fn event_cycle_key(ev: &SignedEvent) -> String {
    let kind = ev.unsigned.kind;
    if is_addressable(kind) || kind == 10002 {
        let d_tag = ev
            .unsigned
            .tags
            .iter()
            .find(|tag| tag.first().map(String::as_str) == Some("d"))
            .and_then(|tag| tag.get(1))
            .cloned()
            .unwrap_or_default();
        format!("{}:{}:{}", kind, ev.unsigned.pubkey, d_tag)
    } else {
        ev.id.clone()
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
                cycle_key: uri.to_string(),
                resolved_kind: 0,
                profile_name: None,
                profile_picture: None,
                event: None,
                rendered: None,
                collapsed: true,
                collapse_reason: Some("dangling".to_string()),
                kind_projection: None,
            };
        };

        match target {
            Target::Profile { name, picture } => EmbedEntry {
                cycle_key: uri.to_string(),
                resolved_kind: 0,
                profile_name: name.clone(),
                profile_picture: picture.clone(),
                event: None,
                rendered: None,
                collapsed: false,
                collapse_reason: None,
                kind_projection: None,
            },
            Target::Event(ev) => self.event_entry(ev),
        }
    }

    fn event_entry(&self, ev: &SignedEvent) -> EmbedEntry {
        let kind = ev.unsigned.kind;
        // Single canonical dispatch: derive the typed per-kind projection from
        // the event itself (article/highlight/short-note/profile/unknown).
        let projection = kind_projection(ev);

        // Unknown/unsupported kind → graceful neutral card (S-E02). This
        // is a context-independent fact (a property of the event kind).
        // kind:9802 (NIP-84 highlight) is a first-class embed kind.
        let known = kind == 1
            || kind == 9802
            || kind == 30023
            || kind == 30000
            || kind == 30003
            || kind == 10002;
        if !known {
            return EmbedEntry {
                cycle_key: event_cycle_key(ev),
                resolved_kind: kind,
                profile_name: None,
                profile_picture: None,
                event: Some(to_json(ev)),
                rendered: None,
                collapsed: true,
                collapse_reason: Some("unsupported".to_string()),
                kind_projection: Some(projection),
            };
        }

        let rendered = Self::render_event_body(ev);
        EmbedEntry {
            cycle_key: event_cycle_key(ev),
            resolved_kind: kind,
            profile_name: None,
            profile_picture: None,
            event: Some(to_json(ev)),
            rendered: Some(rendered),
            collapsed: false,
            collapse_reason: None,
            kind_projection: Some(projection),
        }
    }

    /// Resolve **every** URI transitively reachable from the primary tree
    /// (including through resolved embed bodies). Each URI is resolved
    /// once, unconditionally and fully — the map is a set of resolution
    /// facts. A cyclic reference terminates the transitive walk via the
    /// `out.contains_key` visited check (resolution-dedup, NOT a render
    /// cycle guard): the renderer re-derives PD-015 collapse at walk time.
    pub fn resolve_all(&self, root: &ContentTreeDto) -> BTreeMap<String, EmbedEntry> {
        let mut out = BTreeMap::new();
        self.walk(root, &mut out);
        out
    }

    fn walk(&self, tree: &ContentTreeDto, out: &mut BTreeMap<String, EmbedEntry>) {
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
        S::Mention { uri, .. } | S::EventRef { uri, .. } => out.push(uri.clone()),
        S::MarkdownBlock { node } => collect_node_uris(node, out),
        _ => {}
    }
}

fn collect_node_uris(node: &crate::dto::MarkdownNodeDto, out: &mut Vec<String>) {
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

fn collect_inline_uris(inline: &crate::dto::MarkdownInlineDto, out: &mut Vec<String>) {
    use crate::dto::MarkdownInlineDto as I;
    match inline {
        I::Inline { segment } => collect_uris(segment, out),
        I::Emphasis { children }
        | I::Strong { children }
        | I::Link {
            label: children, ..
        } => {
            for c in children {
                collect_inline_uris(c, out);
            }
        }
        I::Code { .. } | I::Image { .. } | I::SoftBreak | I::HardBreak => {}
    }
}
