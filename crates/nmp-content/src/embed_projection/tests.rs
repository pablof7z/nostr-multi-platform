//! Basic tests for the embed kind projection (F-CR-01).
//!
//! These pin the serde shape and the dispatch logic for the main variants.
//! Full golden fixtures live in nmp-content-fixtures (see plan F-CR-01 / F-CR-12).

use std::collections::BTreeMap;

use nmp_core::refs::{encode_ref_row_delta_batch, RefEventStore, RefRow, RefRowDeltaBatch};
use nmp_core::substrate::KernelEvent;
use nmp_core::typed_projections::{encode_claimed_events, ClaimedEventRow, ClaimedEventsModel};

use super::{
    derive_ref_event_envelopes, derive_ref_event_store_envelopes, resolve_embed_projection,
    EmbedKindProjection, RenderContextWire,
};
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
        relay_provenance: Vec::new(),
    }
}

fn event_row(primary_id: &str, id: &str, kind: u32, content: &str) -> ClaimedEventRow {
    ClaimedEventRow {
        primary_id: primary_id.to_string(),
        id: id.to_string(),
        author_pubkey: "aa".repeat(32),
        kind,
        created_at: 1710000000,
        tags: Vec::new(),
        content: content.to_string(),
        content_tree_bytes: Vec::new(),
        signed_event_json: None,
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
fn resolves_profile_with_display_name_precedence() {
    // #1299: NIP-01/24 precedence is `display_name` > `displayName` > `name`.
    // The old in-Swift resolver had this inverted (`name` first); the Rust
    // resolver is now authoritative and correct.
    let ev = make_event(
        &"aa".repeat(32),
        &"aa".repeat(32),
        0,
        r#"{"name":"snake_name","display_name":"Canonical Name","picture":"https://x.com/a.jpg"}"#,
        vec![],
    );
    let ctx = RenderContext::new();
    match resolve_embed_projection(&ev, &ctx) {
        EmbedKindProjection::Profile(p) => {
            assert_eq!(
                p.display_name.as_deref(),
                Some("Canonical Name"),
                "display_name must win over name (#1299)"
            );
            assert_eq!(p.picture_url.as_deref(), Some("https://x.com/a.jpg"));
        }
        other => panic!("expected Profile, got {other:?}"),
    }
}

#[test]
fn resolves_profile_camel_alias_beats_name() {
    let ev = make_event(
        &"bb".repeat(32),
        &"bb".repeat(32),
        0,
        r#"{"name":"snake","displayName":"Camel Name"}"#,
        vec![],
    );
    let ctx = RenderContext::new();
    match resolve_embed_projection(&ev, &ctx) {
        EmbedKindProjection::Profile(p) => {
            assert_eq!(
                p.display_name.as_deref(),
                Some("Camel Name"),
                "displayName alias must win over name (#1299)"
            );
        }
        other => panic!("expected Profile, got {other:?}"),
    }
}

#[test]
fn resolves_profile_falls_back_to_name() {
    let ev = make_event(
        &"cc".repeat(32),
        &"cc".repeat(32),
        0,
        r#"{"name":"only-name"}"#,
        vec![],
    );
    let ctx = RenderContext::new();
    match resolve_embed_projection(&ev, &ctx) {
        EmbedKindProjection::Profile(p) => {
            assert_eq!(p.display_name.as_deref(), Some("only-name"));
        }
        other => panic!("expected Profile, got {other:?}"),
    }
}

#[test]
fn resolves_profile_empty_content_is_pubkey_only() {
    let ev = make_event(&"dd".repeat(32), &"dd".repeat(32), 0, "", vec![]);
    let ctx = RenderContext::new();
    match resolve_embed_projection(&ev, &ctx) {
        EmbedKindProjection::Profile(p) => {
            assert_eq!(p.pubkey, "dd".repeat(32));
            assert_eq!(
                p.display_name, None,
                "empty content ⇒ no name (D6, no panic)"
            );
            assert_eq!(p.picture_url, None);
        }
        other => panic!("expected Profile, got {other:?}"),
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

#[test]
fn derives_envelope_map_from_refs_event_rows() {
    let event_id = "11".repeat(32);
    let primary_id = format!("30023:{}:my-article", "22".repeat(32));
    let mut row = event_row(&primary_id, &event_id, 30023, "# Article\nBody.");
    row.tags = vec![vec!["d".to_string(), "my-article".to_string()]];
    let rows = BTreeMap::from([(primary_id.clone(), row)]);

    let envelopes = derive_ref_event_envelopes(&rows);
    let env = envelopes
        .get(&primary_id)
        .expect("addressable refs.event row derives an envelope");

    assert_eq!(env.primary_id, primary_id);
    assert_eq!(env.render_context.depth, 0);
    assert_eq!(env.render_context.max_depth, 4);
    match &env.projection {
        EmbedKindProjection::Article(article) => {
            assert_eq!(article.id, event_id);
            assert_eq!(article.d_tag, "my-article");
        }
        other => panic!("expected article projection, got {other:?}"),
    }
}

#[test]
fn deriving_from_updated_rows_replaces_prior_projection() {
    let primary_id = "33".repeat(32);
    let mut rows = BTreeMap::from([(
        primary_id.clone(),
        event_row(&primary_id, &primary_id, 1, "first note"),
    )]);
    let first = derive_ref_event_envelopes(&rows);
    assert!(matches!(
        first.get(&primary_id).map(|env| &env.projection),
        Some(EmbedKindProjection::ShortNote(_))
    ));

    rows.insert(
        primary_id.clone(),
        event_row(&primary_id, &primary_id, 9802, "highlighted text"),
    );
    let updated = derive_ref_event_envelopes(&rows);
    assert!(matches!(
        updated.get(&primary_id).map(|env| &env.projection),
        Some(EmbedKindProjection::Highlight(_))
    ));
}

#[test]
fn deriving_from_empty_materialized_rows_clears_envelopes() {
    let primary_id = "44".repeat(32);
    let rows = BTreeMap::from([(
        primary_id.clone(),
        event_row(&primary_id, &primary_id, 1, "soon cleared"),
    )]);
    assert_eq!(derive_ref_event_envelopes(&rows).len(), 1);

    assert!(
        derive_ref_event_envelopes(&BTreeMap::new()).is_empty(),
        "the derived sidecar mirrors the full current refs.event store"
    );
}

#[test]
fn malformed_rows_fail_closed_without_false_envelope() {
    let key = "55".repeat(32);
    let mut empty_author = event_row(&key, &key, 1, "bad");
    empty_author.author_pubkey.clear();
    let mismatched = event_row("different-primary-id", &key, 1, "bad");
    let rows = BTreeMap::from([
        (key.clone(), empty_author),
        ("66".repeat(32), mismatched),
        ("77".repeat(32), event_row(&"77".repeat(32), "", 1, "bad")),
    ]);

    assert!(
        derive_ref_event_envelopes(&rows).is_empty(),
        "bad rows must be skipped instead of inventing authoritative envelopes"
    );
}

#[test]
fn derives_from_ref_event_store_and_honors_clear_delta() {
    let primary_id = "88".repeat(32);
    let row = event_row(&primary_id, &primary_id, 1, "from store");
    let row_payload = encode_claimed_events(&ClaimedEventsModel {
        entries: vec![(primary_id.clone(), row)],
    });
    let add = encode_ref_row_delta_batch(&RefRowDeltaBatch {
        namespace: "event".to_string(),
        baseline: true,
        rows: vec![RefRow::changed(primary_id.clone(), 1, row_payload)],
    });
    let clear = encode_ref_row_delta_batch(&RefRowDeltaBatch {
        namespace: "event".to_string(),
        baseline: false,
        rows: vec![RefRow::cleared(primary_id.clone(), 2)],
    });

    let mut store = RefEventStore::new();
    store.apply_sidecar(&add, 1, 0);
    assert!(
        derive_ref_event_store_envelopes(&store).contains_key(&primary_id),
        "changed refs.event row derives an envelope"
    );

    store.apply_sidecar(&clear, 1, 0);
    assert!(
        derive_ref_event_store_envelopes(&store).is_empty(),
        "cleared refs.event row removes the derived envelope"
    );
}
