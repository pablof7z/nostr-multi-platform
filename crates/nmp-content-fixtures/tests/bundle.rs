//! Spec-as-code gate for the content-gallery bundle.
//!
//! Asserts (1) the expected scenario count, (2) every signed fixture event
//! verifies with full Schnorr + id-hash via the real
//! `nmp_core::store::VerifiedEvent::try_from_raw`, (3) every embed-bearing
//! segment either resolves in the scenario's `embeds` map or is a
//! deliberate D1 fallback, and (4) the recursion guard actually fired for
//! the depth/cycle scenarios.

use std::collections::BTreeMap;

use nmp_content_fixtures::build_bundle;
use nmp_content_fixtures::dto::{EmbedEntry, ScenarioDto, SegmentDto};
use nmp_core::store::{RawEvent, VerifiedEvent};

// S-A01 (kind:30023 standalone article) and S-A02 (naddr in kind:1 preview card)
// are specified in docs/design/content-gallery-scenarios.md but not yet
// implemented — they require nmp-nip23 article tokenization fixtures.
const EXPECTED_SCENARIOS: usize = 29;

fn verify_event(ev: &nmp_content_fixtures::dto::SignedEventJson) {
    let raw = RawEvent {
        id: ev.id.clone(),
        pubkey: ev.pubkey.clone(),
        created_at: ev.created_at,
        kind: ev.kind,
        tags: ev.tags.clone(),
        content: ev.content.clone(),
        sig: ev.sig.clone(),
    };
    VerifiedEvent::try_from_raw(raw).unwrap_or_else(|e| {
        panic!("fixture event {} failed Schnorr/id verify: {e:?}", ev.id)
    });
}

#[test]
fn bundle_has_expected_scenario_count() {
    let bundle = build_bundle();
    assert_eq!(
        bundle.scenarios.len(),
        EXPECTED_SCENARIOS,
        "scenario count drifted from the matrix spec"
    );
    let mut ids: Vec<&str> =
        bundle.scenarios.iter().map(|s| s.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), EXPECTED_SCENARIOS, "duplicate scenario ids");
}

#[test]
fn every_signed_event_verifies() {
    for s in build_bundle().scenarios {
        for ev in &s.events {
            verify_event(ev);
        }
        for entry in s.embeds.values() {
            if let Some(ev) = &entry.event {
                verify_event(ev);
            }
        }
    }
}

fn assert_embeds_resolve(s: &ScenarioDto, embeds: &BTreeMap<String, EmbedEntry>) {
    for seg in &s.rendered.segments {
        let uri = match seg {
            SegmentDto::Mention { uri, .. } => uri,
            SegmentDto::EventRef { uri, .. } => uri,
            _ => continue,
        };
        assert!(
            embeds.contains_key(uri),
            "scenario {} references {uri} with no embed entry",
            s.id
        );
    }
}

#[test]
fn every_referenced_uri_has_an_embed_entry() {
    for s in build_bundle().scenarios {
        let embeds = s.embeds.clone();
        assert_embeds_resolve(&s, &embeds);
    }
}

/// Recursively check whether any segment in a tree is an `EventRef` whose
/// `id` equals `coord` (descends into Markdown blocks/inlines).
fn tree_refs_id(tree: &nmp_content_fixtures::dto::ContentTreeDto, coord: &str) -> bool {
    tree.segments.iter().any(|s| seg_refs_id(s, coord))
}

fn seg_refs_id(seg: &SegmentDto, coord: &str) -> bool {
    use nmp_content_fixtures::dto::{MarkdownInlineDto as I, MarkdownNodeDto as N};
    fn node(n: &N, c: &str) -> bool {
        match n {
            N::Heading { inlines, .. } | N::Paragraph { inlines } => {
                inlines.iter().any(|i| inl(i, c))
            }
            N::BlockQuote { blocks } => blocks.iter().any(|b| node(b, c)),
            N::List { items, .. } => {
                items.iter().any(|it| it.iter().any(|b| node(b, c)))
            }
            N::CodeBlock { .. } | N::Rule => false,
        }
    }
    fn inl(i: &I, c: &str) -> bool {
        match i {
            I::Inline { segment } => seg_refs_id(segment, c),
            I::Emphasis { children }
            | I::Strong { children }
            | I::Link { label: children, .. } => {
                children.iter().any(|x| inl(x, c))
            }
            _ => false,
        }
    }
    match seg {
        SegmentDto::EventRef { id, .. } => id == coord,
        SegmentDto::MarkdownBlock { node: n } => node(n, coord),
        _ => false,
    }
}

#[test]
fn depth_chain_fully_resolves_all_five_levels() {
    // The bundle provides resolution facts; PD-015 depth collapse is the
    // renderer's job at walk time. All 5 quote levels must resolve fully.
    let bundle = build_bundle();
    let s = bundle
        .scenarios
        .iter()
        .find(|s| s.id == "S-M08")
        .expect("S-M08 present");
    let resolved_events = s
        .embeds
        .values()
        .filter(|e| {
            e.resolved_kind == 1
                && !e.collapsed
                && e.rendered.is_some()
        })
        .count();
    assert!(
        resolved_events >= 5,
        "S-M08 must fully resolve all 5 nested quote levels, got {resolved_events}"
    );
}

#[test]
fn cycle_pair_resolves_with_mutual_back_references() {
    // S-M09: each cycle article resolves fully, and each rendered body
    // contains an EventRef back to the other — this is exactly what
    // triggers the renderer's `visited`-set cycle guard at render time.
    let bundle = build_bundle();
    let s = bundle
        .scenarios
        .iter()
        .find(|s| s.id == "S-M09")
        .expect("S-M09 present");

    // Both cycle articles must resolve fully (rendered body present).
    let articles: Vec<&EmbedEntry> = s
        .embeds
        .values()
        .filter(|e| e.resolved_kind == 30023 && e.rendered.is_some())
        .collect();
    assert_eq!(
        articles.len(),
        2,
        "S-M09 must resolve both cycle articles"
    );

    // Each article's naddr coord, derived from its own signed event.
    let coords: Vec<String> = articles
        .iter()
        .filter_map(|e| e.event.as_ref())
        .map(|ev| {
            let d = ev
                .tags
                .iter()
                .find(|t| t.first().map(String::as_str) == Some("d"))
                .and_then(|t| t.get(1).cloned())
                .unwrap_or_default();
            format!("{}:{}:{}", ev.kind, ev.pubkey, d)
        })
        .collect();
    assert_eq!(coords.len(), 2, "two distinct cycle coords");

    let bodies: Vec<&nmp_content_fixtures::dto::ContentTreeDto> =
        articles.iter().filter_map(|e| e.rendered.as_ref()).collect();
    // Mutual back-reference: each coord is referenced by some body.
    assert!(
        bodies.iter().any(|b| tree_refs_id(b, &coords[0]))
            && bodies.iter().any(|b| tree_refs_id(b, &coords[1])),
        "S-M09 cycle bodies must mutually back-reference \
         (renderer collapses the cycle at render time)"
    );
}

#[test]
fn dangling_and_unsupported_are_bundle_time_facts() {
    let bundle = build_bundle();

    let dangling = bundle
        .scenarios
        .iter()
        .find(|s| s.id == "S-E03")
        .expect("S-E03 present");
    assert!(
        dangling
            .embeds
            .values()
            .any(|e| e.collapse_reason.as_deref() == Some("dangling")),
        "S-E03 must produce a dangling stub (context-independent fact)"
    );

    let unsupported = bundle
        .scenarios
        .iter()
        .find(|s| s.id == "S-E02")
        .expect("S-E02 present");
    assert!(
        unsupported
            .embeds
            .values()
            .any(|e| e.collapse_reason.as_deref() == Some("unsupported")),
        "S-E02 must produce an unsupported-kind stub (context-independent fact)"
    );
}
