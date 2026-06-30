//! Cross-protocol engagement reference counters (#2512).
//!
//! `nmp-store` (Layer 1) exposes only a generic, protocol-noun-free reference
//! counter: per target event, counts bucketed by opaque ids the store never
//! interprets. This module owns the protocol-aware half — which kinds count,
//! how NIP-10 reply markers pick the target, and what each bucket *means*
//! (reply / reaction / repost / zap) — because cross-protocol social-relation
//! aggregation is the sole responsibility of `nmp-relations` (Layer 4) per
//! `docs/architecture/crate-boundaries.md` §8.
//!
//! The seam mirrors the FTS one exactly: a protocol-aware spec is compiled into
//! an opaque [`nmp_store::ReferenceClassifyFn`] and injected into the store at
//! composition time via
//! [`nmp_store::EventStore::install_reference_counter_classifier`]; the typed
//! [`TargetInteractionCounts`] summary is read back over the generic counts.

use std::sync::Arc;

use nmp_store::{
    EventId, EventStore, ReferenceBucketId, ReferenceClassifyFn, StoreError, TargetReferenceCounts,
};

// ─── Opaque buckets ─────────────────────────────────────────────────────────
//
// The store keys on the discriminant alone and assigns it no meaning; only this
// module maps a bucket back to its protocol noun.

const REPLY: ReferenceBucketId = ReferenceBucketId::new(1, "reply");
const REACTION: ReferenceBucketId = ReferenceBucketId::new(2, "reaction");
const REPOST: ReferenceBucketId = ReferenceBucketId::new(3, "repost");
const ZAP: ReferenceBucketId = ReferenceBucketId::new(4, "zap");

// Counted kinds. These live HERE (L4), never in the storage layer (#2512).
const KIND_NOTE: u32 = 1; // NIP-10 reply (kind:1 e-tag with reply/root/bare marker)
const KIND_REACTION: u32 = 7; // NIP-25
const KIND_REPOST: u32 = 6; // NIP-18
const KIND_ZAP_RECEIPT: u32 = 9735; // NIP-57

/// Aggregated engagement counts for one target event.
///
/// The typed cross-protocol summary. Lives in `nmp-relations` (L4), not in the
/// storage layer — `nmp-store` exposes only the noun-free
/// [`TargetReferenceCounts`] this is projected from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TargetInteractionCounts {
    /// kind:1 events that reply-/root-/bare-`e`-tag the target (NIP-10).
    pub replies: u64,
    /// kind:7 reactions e-tagging the target (NIP-25).
    pub reactions: u64,
    /// kind:6 reposts e-tagging the target (NIP-18).
    pub reposts: u64,
    /// kind:9735 zap receipts e-tagging the target (NIP-57).
    pub zaps: u64,
}

impl TargetInteractionCounts {
    /// Project the store's opaque bucket counts onto the typed engagement nouns.
    #[must_use]
    pub fn from_reference_counts(rc: &TargetReferenceCounts) -> Self {
        Self {
            replies: rc.get(REPLY),
            reactions: rc.get(REACTION),
            reposts: rc.get(REPOST),
            zaps: rc.get(ZAP),
        }
    }
}

/// The engagement classifier, compiled into the store's opaque closure.
///
/// Inject it once at composition time via
/// [`install_engagement_reference_counters`].
#[must_use]
pub fn engagement_reference_classifier() -> Arc<ReferenceClassifyFn> {
    let f: Arc<ReferenceClassifyFn> = Arc::new(classify);
    f
}

/// Install the engagement reference-counter classifier into `store`. Call once
/// at composition time, after the store exists (mirrors
/// `SearchScopeRegistry::install_into` for FTS).
pub fn install_engagement_reference_counters(store: &dyn EventStore) {
    store.install_reference_counter_classifier(engagement_reference_classifier());
}

/// Read the typed engagement counts for `target`, projected from the store's
/// generic reference counts.
pub fn engagement_counts(
    store: &dyn EventStore,
    target: &EventId,
) -> Result<TargetInteractionCounts, StoreError> {
    let rc = store.reference_counts(target)?;
    Ok(TargetInteractionCounts::from_reference_counts(&rc))
}

/// Classify an event by `kind` + `tags` into its `(bucket, target_event_id_hex)`
/// reference edge, or `None` if it is not a counted engagement.
///
/// kind:1 honours NIP-10 reply-marker precedence ("reply" > "root" > first bare
/// `e`); the other kinds count against their first `e` tag.
fn classify(kind: u32, tags: &[Vec<String>]) -> Option<(ReferenceBucketId, String)> {
    match kind {
        KIND_NOTE => classify_reply(tags).map(|id| (REPLY, id)),
        KIND_REACTION => first_e_tag(tags).map(|id| (REACTION, id)),
        KIND_REPOST => first_e_tag(tags).map(|id| (REPOST, id)),
        KIND_ZAP_RECEIPT => first_e_tag(tags).map(|id| (ZAP, id)),
        _ => None,
    }
}

/// NIP-10 reply target precedence: "reply" > "root" > first bare `e` tag.
fn classify_reply(tags: &[Vec<String>]) -> Option<String> {
    let mut reply_id: Option<String> = None;
    let mut root_id: Option<String> = None;
    let mut first_id: Option<String> = None;

    for tag in tags {
        if tag.len() < 2 || tag[0] != "e" {
            continue;
        }
        let id = tag[1].clone();
        let marker = tag.get(3).map(String::as_str).unwrap_or("");
        if marker == "reply" && reply_id.is_none() {
            reply_id = Some(id);
        } else if marker == "root" && root_id.is_none() {
            root_id = Some(id);
        } else if first_id.is_none() && marker != "reply" && marker != "root" {
            first_id = Some(id);
        }
    }
    reply_id.or(root_id).or(first_id)
}

/// The first `e` tag value, or `None`.
fn first_e_tag(tags: &[Vec<String>]) -> Option<String> {
    tags.iter()
        .find(|tag| tag.len() >= 2 && tag[0] == "e")
        .map(|tag| tag[1].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};

    // ─── classify() unit coverage (ported from the old nmp-store classifier) ──

    fn etag(id: &str) -> Vec<String> {
        vec!["e".into(), id.into()]
    }
    fn etag_m(id: &str, marker: &str) -> Vec<String> {
        vec!["e".into(), id.into(), "wss://r/".into(), marker.into()]
    }

    #[test]
    fn kind1_reply_marker_wins_over_root_and_bare() {
        let tags = vec![etag("bare"), etag_m("rootid", "root"), etag_m("replyid", "reply")];
        assert_eq!(classify(KIND_NOTE, &tags), Some((REPLY, "replyid".into())));
    }

    #[test]
    fn kind1_root_wins_over_bare() {
        let tags = vec![etag("bare"), etag_m("rootid", "root")];
        assert_eq!(classify(KIND_NOTE, &tags), Some((REPLY, "rootid".into())));
    }

    #[test]
    fn kind1_bare_etag_is_reply_target() {
        assert_eq!(classify(KIND_NOTE, &[etag("deadbeef")]), Some((REPLY, "deadbeef".into())));
    }

    #[test]
    fn kind1_no_etag_is_none() {
        assert!(classify(KIND_NOTE, &[vec!["p".into(), "x".into()]]).is_none());
    }

    #[test]
    fn reaction_repost_zap_first_etag() {
        assert_eq!(classify(KIND_REACTION, &[etag("a")]), Some((REACTION, "a".into())));
        assert_eq!(classify(KIND_REPOST, &[etag("b")]), Some((REPOST, "b".into())));
        assert_eq!(classify(KIND_ZAP_RECEIPT, &[etag("c")]), Some((ZAP, "c".into())));
    }

    #[test]
    fn non_engagement_kinds_are_none() {
        assert!(classify(0, &[etag("x")]).is_none());
        assert!(classify(3, &[etag("x")]).is_none());
        assert!(classify(30023, &[etag("x")]).is_none());
    }

    #[test]
    fn buckets_are_distinct() {
        // The four engagement nouns occupy four distinct discriminants.
        let ds: std::collections::BTreeSet<u8> = [REPLY, REACTION, REPOST, ZAP]
            .into_iter()
            .map(ReferenceBucketId::discriminant)
            .collect();
        assert_eq!(ds.len(), 4);
    }

    // ─── End-to-end: install the classifier into a real store ─────────────────

    const TARGET: &str = "0000000000000000000000000000000000000000000000000000000000000001";
    const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn target_id() -> EventId {
        let bytes = (0..32)
            .map(|i| u8::from_str_radix(&TARGET[i * 2..i * 2 + 2], 16).unwrap())
            .collect::<Vec<_>>();
        let mut id = [0u8; 32];
        id.copy_from_slice(&bytes);
        id
    }

    fn ev(id_hex: &str, kind: u32, tags: Vec<Vec<String>>, created_at: u64) -> VerifiedEvent {
        VerifiedEvent::from_raw_unchecked(RawEvent {
            id: id_hex.to_string(),
            pubkey: AUTHOR.to_string(),
            created_at,
            kind,
            tags,
            content: String::new(),
            sig: "0".repeat(128),
        })
    }

    #[test]
    fn engagement_counts_flow_end_to_end() {
        let store = MemEventStore::new();
        install_engagement_reference_counters(&store);
        let relay = "wss://r/".to_string();

        // One reply, two reactions, one repost, one zap — all e-tagging TARGET.
        store.insert(ev(&"11".repeat(32), KIND_NOTE, vec![etag(TARGET)], 1000), &relay, 1_000_000).unwrap();
        store.insert(ev(&"21".repeat(32), KIND_REACTION, vec![etag(TARGET)], 1001), &relay, 1_000_001).unwrap();
        store.insert(ev(&"22".repeat(32), KIND_REACTION, vec![etag(TARGET)], 1002), &relay, 1_000_002).unwrap();
        store.insert(ev(&"31".repeat(32), KIND_REPOST, vec![etag(TARGET)], 1003), &relay, 1_000_003).unwrap();
        store.insert(ev(&"41".repeat(32), KIND_ZAP_RECEIPT, vec![etag(TARGET)], 1004), &relay, 1_000_004).unwrap();

        let counts = engagement_counts(&store, &target_id()).unwrap();
        assert_eq!(
            counts,
            TargetInteractionCounts { replies: 1, reactions: 2, reposts: 1, zaps: 1 }
        );
    }

    #[test]
    fn without_install_counts_are_zero() {
        let store = MemEventStore::new();
        let relay = "wss://r/".to_string();
        store.insert(ev(&"11".repeat(32), KIND_REACTION, vec![etag(TARGET)], 1000), &relay, 1_000_000).unwrap();
        // No classifier installed → the store maintains nothing.
        assert_eq!(engagement_counts(&store, &target_id()).unwrap(), TargetInteractionCounts::default());
    }
}
