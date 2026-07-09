//! Shared fixtures for the `open_composite_feed` driving-example integration
//! tests (`composite_feed_driving_example.rs` and
//! `composite_feed_delivered_ref_order_independence.rs`; split out for
//! file-size discipline — pre-#3086-merge polish). Mirrors the
//! `reduced_source_relay_e2e/support.rs` convention of a shared `#[path]`
//! module every sibling integration-test binary in this crate pulls in.

use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_feed::{FeedRowContext, LaneMapping, LaneMappingId, LaneMappingRegistry, MappedPayload, MappedRow};
use nmp_nip23::KIND_LONG_FORM_ARTICLE;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

pub(crate) const KIND_COMMENT: u16 = 1111;
pub(crate) const KIND_REPOST: u16 = 16;
pub(crate) const TEST_DIRECT_MAPPING_ID: &str = "test.3086.article_direct";

pub(crate) fn article_event(keys: &Keys, d: &str, created_at: u64, body: &str) -> nostr::Event {
    EventBuilder::new(Kind::from(KIND_LONG_FORM_ARTICLE as u16), body)
        .tags(vec![Tag::parse(["d", d]).expect("d tag")])
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign article")
}

pub(crate) fn comment_event(keys: &Keys, article_pk: &str, d: &str, created_at: u64) -> nostr::Event {
    let coord = format!("{KIND_LONG_FORM_ARTICLE}:{article_pk}:{d}");
    let kind_str = KIND_LONG_FORM_ARTICLE.to_string();
    EventBuilder::new(Kind::from(KIND_COMMENT), "nice article")
        .tags(vec![
            Tag::parse(["A", coord.as_str()]).expect("A tag"),
            Tag::parse(["K", kind_str.as_str()]).expect("K tag"),
            Tag::parse(["a", coord.as_str()]).expect("a tag"),
            Tag::parse(["k", kind_str.as_str()]).expect("k tag"),
        ])
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign comment")
}

pub(crate) fn repost_event(keys: &Keys, article_pk: &str, d: &str, created_at: u64) -> nostr::Event {
    let coord = format!("{KIND_LONG_FORM_ARTICLE}:{article_pk}:{d}");
    let kind_str = KIND_LONG_FORM_ARTICLE.to_string();
    EventBuilder::new(Kind::from(KIND_REPOST), "")
        .tags(vec![
            Tag::parse(["a", coord.as_str()]).expect("a tag"),
            Tag::parse(["k", kind_str.as_str()]).expect("k tag"),
        ])
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign repost")
}

/// A real app/nip23 composition root's OWN coordinate-keyed "direct" mapping
/// for its address-replaceable article kind (mirrors
/// `composite_compiler_tests.rs`'s `direct_article_mapping`, now registered
/// through the REAL [`LaneMappingRegistry`] this crate's `NmpApp` test seam
/// consumes, rather than a hand-built closure passed straight to `FlatFeed`).
pub(crate) fn direct_article_mapping() -> LaneMapping {
    Arc::new(|event: &KernelEvent| {
        let Some(d) = event
            .tags
            .iter()
            .find(|tag| tag.first().map(String::as_str) == Some("d"))
            .and_then(|tag| tag.get(1))
        else {
            return Vec::new();
        };
        vec![MappedRow {
            canonical_row_id: format!("{}:{}:{}", event.kind, event.author, d),
            payload: MappedPayload::FromEvent,
            context: vec![FeedRowContext::Authored],
            refs: Vec::new(),
        }]
    })
}

/// The registry every test in these files shares: `feed.authored` (unused
/// here, pre-installed for parity with the production registry
/// `NmpApp::open_composite_feed` builds), the REAL `nip18.target`/`nip22.root`
/// production mappings, and the test-local coordinate-keyed direct mapping.
pub(crate) fn test_registry() -> LaneMappingRegistry {
    let registry = LaneMappingRegistry::new();
    registry.register(
        LaneMappingId(TEST_DIRECT_MAPPING_ID.to_string()),
        direct_article_mapping(),
    );
    registry.register(
        LaneMappingId(nmp_nip18::NIP18_TARGET_MAPPING_ID.to_string()),
        nmp_nip18::nip18_target_mapping(),
    );
    registry.register(
        LaneMappingId(nmp_nip22::NIP22_ROOT_MAPPING_ID.to_string()),
        nmp_nip22::nip22_root_mapping(),
    );
    registry
}
