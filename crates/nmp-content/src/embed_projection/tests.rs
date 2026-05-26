//! Basic tests for the embed kind projection (F-CR-01).
//!
//! These pin the serde shape and the dispatch logic for the main variants.
//! Full golden fixtures live in nmp-content-fixtures (see plan F-CR-01 / F-CR-12).

use nmp_core::substrate::KernelEvent;

use super::{resolve_embed_projection, EmbedKindProjection, RenderContextWire};
use crate::context::RenderContext;

fn make_event(
    id: &str,
    author: &str,
    kind: u32,
    content: &str,
    tags: Vec<Vec<String>>,
) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind,
        created_at: 1710000000,
        tags,
        content: content.to_string(),
    }
}

#[test]
fn resolves_short_note() {
    let ev = make_event(
        "note123",
        "aa".repeat(32).as_str(),
        1,
        "Hello nostr",
        vec![],
    );
    let ctx = RenderContext::new();
    let proj = resolve_embed_projection(&ev, &ctx);

    match proj {
        EmbedKindProjection::ShortNote(n) => {
            assert_eq!(n.id, "note123");
            assert_eq!(n.author_pubkey, "aa".repeat(32));
            assert!(!n.content_tree.nodes.is_empty() || n.content_tree.roots.is_empty());
        }
        _ => panic!("expected ShortNote"),
    }
}

#[test]
fn resolves_article_with_d_tag() {
    let tags = vec![vec!["d".to_string(), "my-article".to_string()]];
    let ev = make_event(
        "art456",
        "bb".repeat(32).as_str(),
        30023,
        "# My Article\nBody here.",
        tags,
    );
    let ctx = RenderContext::new();
    let proj = resolve_embed_projection(&ev, &ctx);

    match proj {
        EmbedKindProjection::Article(a) => {
            assert_eq!(a.d_tag, "my-article");
            assert_eq!(a.id, "art456");
        }
        _ => panic!("expected Article"),
    }
}

#[test]
fn resolves_unknown_kind_with_raw_tags() {
    let tags = vec![vec!["price".to_string(), "42".to_string()]];
    let ev = make_event(
        "unk789",
        "cc".repeat(32).as_str(),
        30402,
        "Classified ad",
        tags,
    );
    let ctx = RenderContext::new();
    let proj = resolve_embed_projection(&ev, &ctx);

    match proj {
        EmbedKindProjection::Unknown(u) => {
            assert_eq!(u.kind, 30402);
            assert_eq!(u.tags.len(), 1);
            assert_eq!(u.tags[0][0], "price");
        }
        _ => panic!("expected Unknown"),
    }
}

#[test]
fn render_context_wire_roundtrip() {
    let mut ctx = RenderContext::with_max_depth(3);
    ctx.visited.push("deadbeef".to_string());

    let wire = RenderContextWire::from(&ctx);
    assert_eq!(wire.depth, 0);
    assert_eq!(wire.max_depth, 3);
    assert_eq!(wire.visited, vec!["deadbeef".to_string()]);

    let back: RenderContext = (&wire).into();
    assert_eq!(back.max_depth, 3);
    assert_eq!(back.visited.len(), 1);
}
