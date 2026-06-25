//! Spec-as-code gate for the content-gallery bundle.
//!
//! Asserts (1) the expected scenario count, (2) every signed fixture event
//! verifies with full Schnorr + id-hash via the real
//! `nmp_store::VerifiedEvent::try_from_raw`, (3) every embed-bearing
//! segment either resolves in the scenario's `embeds` map or is a
//! deliberate D1 fallback, and (4) the recursion guard actually fired for
//! the depth/cycle scenarios.

use std::collections::BTreeMap;

use nmp_content::EmbedKindProjection;
use nmp_content_fixtures::build_bundle;
use nmp_content_fixtures::dto::{EmbedEntry, ScenarioDto, SegmentDto};
use nmp_store::{RawEvent, VerifiedEvent};

const EXPECTED_SCENARIOS: usize = 43; // +5 F-CR-12 (S-M10…S-M14)

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
    VerifiedEvent::try_from_raw(raw)
        .unwrap_or_else(|e| panic!("fixture event {} failed Schnorr/id verify: {e:?}", ev.id));
}

#[test]
fn bundle_has_expected_scenario_count() {
    let bundle = build_bundle();
    assert_eq!(
        bundle.scenarios.len(),
        EXPECTED_SCENARIOS,
        "scenario count drifted from the matrix spec"
    );
    let mut ids: Vec<&str> = bundle.scenarios.iter().map(|s| s.id.as_str()).collect();
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

#[test]
fn every_embed_entry_carries_a_rust_owned_cycle_key() {
    for scenario in build_bundle().scenarios {
        for (uri, entry) in scenario.embeds {
            assert!(
                !entry.cycle_key.is_empty(),
                "scenario {} embed {uri} has an empty cycle_key",
                scenario.id
            );
        }
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
            N::List { items, .. } => items.iter().any(|it| it.iter().any(|b| node(b, c))),
            N::CodeBlock { .. } | N::Rule => false,
        }
    }
    fn inl(i: &I, c: &str) -> bool {
        match i {
            I::Inline { segment } => seg_refs_id(segment, c),
            I::Emphasis { children }
            | I::Strong { children }
            | I::Link {
                label: children, ..
            } => children.iter().any(|x| inl(x, c)),
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
        .filter(|e| e.resolved_kind == 1 && !e.collapsed && e.rendered.is_some())
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
    assert_eq!(articles.len(), 2, "S-M09 must resolve both cycle articles");

    let coords: Vec<String> = articles
        .iter()
        .map(|entry| entry.cycle_key.clone())
        .collect();
    assert_eq!(coords.len(), 2, "two distinct cycle coords");
    assert!(
        coords.iter().all(|coord| coord.starts_with("30023:")),
        "article cycle keys must be opaque naddr coords: {coords:?}"
    );

    let bodies: Vec<&nmp_content_fixtures::dto::ContentTreeDto> = articles
        .iter()
        .filter_map(|e| e.rendered.as_ref())
        .collect();
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

/// Every resolved event embed carries the canonical typed per-kind projection
/// (#1998). This is the single shape native registries dispatch on, replacing
/// the retired hand-rolled `article` / `list` fields. The variant must match
/// the resolved kind so article → Article card, highlight → Highlight card,
/// short note → ShortNote card, and NIP-51 lists / unknown kinds fall through
/// to the Unknown variant (which carries the raw tags native list renderers
/// read).
#[test]
fn resolved_embeds_carry_typed_kind_projection() {
    for scenario in build_bundle().scenarios {
        for (uri, entry) in scenario.embeds {
            // Dangling stubs and bare profile targets have no underlying event
            // and therefore no kind projection.
            let Some(event) = &entry.event else {
                assert!(
                    entry.kind_projection.is_none(),
                    "scenario {} embed {uri}: no event but a kind_projection present",
                    scenario.id
                );
                continue;
            };

            let projection = entry.kind_projection.as_ref().unwrap_or_else(|| {
                panic!(
                    "scenario {} embed {uri}: resolved event missing kind_projection",
                    scenario.id
                )
            });

            // The dispatched variant must agree with the resolved kind — this
            // is the kind-dispatch contract every platform registry relies on.
            match (entry.resolved_kind, projection) {
                (1, EmbedKindProjection::ShortNote(_))
                | (9802, EmbedKindProjection::Highlight(_))
                | (30023, EmbedKindProjection::Article(_))
                | (0, EmbedKindProjection::Profile(_)) => {}
                // NIP-51 lists (30000 / 30003 / 10002) + any other kind have no
                // registered typed projection → Unknown carrying raw tags.
                (_, EmbedKindProjection::Unknown(u)) => {
                    assert_eq!(
                        u.kind, event.kind,
                        "scenario {} embed {uri}: Unknown projection kind mismatch",
                        scenario.id
                    );
                }
                (kind, other) => panic!(
                    "scenario {} embed {uri}: kind {kind} dispatched to wrong projection variant {other:?}",
                    scenario.id
                ),
            }
        }
    }
}

/// Article, list, and quote embeds each render through the typed kind-dispatch
/// projection — a focused spec-as-code gate for the three legacy shapes
/// (article header / NIP-51 list / short-note quote) the migration retired.
#[test]
fn article_list_quote_embeds_dispatch_through_typed_projection() {
    let bundle = build_bundle();

    // Article (kind:30023): S-M10 quotes a long-form article via naddr.
    let article = bundle
        .scenarios
        .iter()
        .find(|s| s.id == "S-M10")
        .expect("S-M10 present");
    assert!(
        article.embeds.values().any(|e| matches!(
            &e.kind_projection,
            Some(EmbedKindProjection::Article(a)) if a.title.as_deref() == Some("Backpressure Is A Feature")
        )),
        "S-M10 article embed must dispatch to a typed Article projection"
    );

    // NIP-51 list (kind:30000): S-A03 follow set resolves to an Unknown
    // projection whose raw tags carry the list members + title.
    let list = bundle
        .scenarios
        .iter()
        .find(|s| s.id == "S-A03")
        .expect("S-A03 present");
    assert!(
        list.embeds.values().any(|e| matches!(
            &e.kind_projection,
            Some(EmbedKindProjection::Unknown(u))
                if u.kind == 30000
                    && u.tags.iter().any(|t| t.first().map(String::as_str) == Some("p"))
                    && u.tags.iter().any(|t| t.first().map(String::as_str) == Some("title"))
        )),
        "S-A03 list embed must dispatch to an Unknown projection carrying raw list tags"
    );

    // Short-note quote (kind:1): S-M12 chains kind:1 notes.
    let quote = bundle
        .scenarios
        .iter()
        .find(|s| s.id == "S-M12")
        .expect("S-M12 present");
    assert!(
        quote.embeds.values().any(|e| matches!(
            &e.kind_projection,
            Some(EmbedKindProjection::ShortNote(_))
        )),
        "S-M12 quote embeds must dispatch to typed ShortNote projections"
    );
}
