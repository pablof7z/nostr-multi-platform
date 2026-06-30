//! #2512 — composition-root regression guard for engagement reference counts.
//!
//! The store no longer self-classifies engagement; the cross-protocol
//! classifier (`nmp-relations`, L4) must be wired in by a production
//! composition root or new stores silently maintain ZERO counts. This proves
//! the native runtime's composition root (`app_ctor`) composes the engagement
//! classifier and that, once installed exactly as `apply_to_kernel` installs
//! it, a real store returns NON-EMPTY engagement buckets for a known e-tag
//! reference.
//!
//! `apply_to_kernel` itself (the seam that installs this classifier into the
//! kernel store on start + every `Reset`) is covered in `nmp-core`'s
//! `actor::config` tests; here we pin the *native root's choice of classifier*.

use nmp_relations::{engagement_counts, TargetInteractionCounts};
use nmp_store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};

const TARGET: &str = "0000000000000000000000000000000000000000000000000000000000000001";
const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn target_id() -> [u8; 32] {
    let mut id = [0u8; 32];
    for (i, slot) in id.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&TARGET[i * 2..i * 2 + 2], 16).unwrap();
    }
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

/// The classifier the native composition root installs must, when installed
/// into a real store (the exact `EventStore::install_reference_counter_classifier`
/// call `apply_to_kernel` makes), maintain engagement buckets at ingest. A
/// reaction, repost, zap, and reply against one target must all be counted —
/// proving the root composes engagement counting, not the prior silent-zero
/// regression where no classifier was installed at all.
#[test]
fn native_root_composes_engagement_reference_counter() {
    let store = MemEventStore::new();
    store.install_reference_counter_classifier(crate::app_ctor::composed_reference_classifier());

    let relay = "wss://r/".to_string();
    let etag = |id: &str| vec!["e".to_string(), id.to_string()];

    // kind:1 reply, kind:7 reaction, kind:6 repost, kind:9735 zap → all e-tag TARGET.
    store.insert(ev(&"11".repeat(32), 1, vec![etag(TARGET)], 1000), &relay, 1_000_000).unwrap();
    store.insert(ev(&"21".repeat(32), 7, vec![etag(TARGET)], 1001), &relay, 1_000_001).unwrap();
    store.insert(ev(&"31".repeat(32), 6, vec![etag(TARGET)], 1002), &relay, 1_000_002).unwrap();
    store.insert(ev(&"41".repeat(32), 9735, vec![etag(TARGET)], 1003), &relay, 1_000_003).unwrap();

    let counts = engagement_counts(&store, &target_id()).unwrap();
    assert_eq!(
        counts,
        TargetInteractionCounts { replies: 1, reactions: 1, reposts: 1, zaps: 1 },
        "native composition root must compose a classifier that maintains engagement buckets",
    );
}

/// Without the root's classifier the same store maintains nothing — the exact
/// regression #2512 guards against (composition forgot to install).
#[test]
fn store_without_composed_classifier_counts_zero() {
    let store = MemEventStore::new();
    let relay = "wss://r/".to_string();
    store
        .insert(ev(&"21".repeat(32), 7, vec![vec!["e".into(), TARGET.into()]], 1000), &relay, 1_000_000)
        .unwrap();
    assert_eq!(
        engagement_counts(&store, &target_id()).unwrap(),
        TargetInteractionCounts::default(),
        "no classifier installed → store maintains zero engagement counts",
    );
}
