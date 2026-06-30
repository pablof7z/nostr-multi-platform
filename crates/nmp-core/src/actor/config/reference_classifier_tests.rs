//! #2512 — seam test: the composition-root-injected reference classifier on
//! `ActorConfigSources` must be installed into the kernel's store by
//! `apply_to_kernel` (the same path that installs the FTS scope registry, and
//! the path a `Reset` re-runs against the fresh store). This guards the wiring
//! the native/browser composition roots rely on: set the field → counts are
//! maintained at ingest. A synthetic, kind-agnostic classifier is used because
//! `nmp-core` (L3) must never name the engagement nouns (`nmp-relations`, L4).

use std::sync::{Arc, Mutex};

use nmp_store::{RawEvent, ReferenceBucketId, ReferenceClassifyFn, VerifiedEvent};

use crate::actor::{ActorConfig, ActorConfigSources};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

const TARGET: &str = "0000000000000000000000000000000000000000000000000000000000000001";
const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TEST_BUCKET: ReferenceBucketId = ReferenceBucketId::new(9, "test");

fn target_id() -> [u8; 32] {
    let mut id = [0u8; 32];
    for (i, slot) in id.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&TARGET[i * 2..i * 2 + 2], 16).unwrap();
    }
    id
}

/// A synthetic, protocol-noun-free classifier: kind:7's first `e` tag → the
/// opaque test bucket. The point is the *seam* (field → store), not engagement
/// semantics (those are `nmp-relations`' tests).
fn synthetic_classifier() -> Arc<ReferenceClassifyFn> {
    Arc::new(|kind: u32, tags: &[Vec<String>]| {
        if kind == 7 {
            tags.iter()
                .find(|t| t.len() >= 2 && t[0] == "e")
                .map(|t| (TEST_BUCKET, t[1].clone()))
        } else {
            None
        }
    })
}

fn reaction_event() -> VerifiedEvent {
    VerifiedEvent::from_raw_unchecked(RawEvent {
        id: "21".repeat(32),
        pubkey: AUTHOR.to_string(),
        created_at: 1000,
        kind: 7,
        tags: vec![vec!["e".to_string(), TARGET.to_string()]],
        content: String::new(),
        sig: "0".repeat(128),
    })
}

/// Build an `ActorConfigSources` with everything inert except the injected
/// `reference_counter_classifier`, mirroring the shape the native runtime
/// composes.
fn sources_with_classifier(classifier: Option<Arc<ReferenceClassifyFn>>) -> ActorConfig {
    ActorConfigSources {
        storage_path: crate::slots::new_storage_path_slot(),
        coverage_hook: Arc::new(Mutex::new(None)),
        req_frame_interceptor: crate::substrate::new_req_frame_interceptor_slot(),
        host_op_handler: crate::substrate::new_host_op_handler_slot(),
        relay_text_interceptor: crate::substrate::new_relay_text_interceptor_slot(),
        relay_connected_hook: crate::substrate::new_relay_connected_hook_slot(),
        ingest_dispatcher: Arc::new(std::sync::RwLock::new(
            crate::substrate::EventIngestDispatcher::new(),
        )),
        search_scope_registry: Arc::new(crate::substrate::SearchScopeRegistry::new()),
        reference_counter_classifier: classifier,
        dm_inbox_relays: Arc::new(Mutex::new(crate::substrate::empty_dm_inbox_relay_lookup())),
        profile_lookup: Arc::new(Mutex::new(crate::substrate::empty_profile_lookup())),
        contacts_lookup: Arc::new(Mutex::new(crate::substrate::empty_contacts_lookup())),
        blocked_relays: Arc::new(Mutex::new(crate::substrate::empty_blocked_relay_lookup())),
        bootstrap_self_kinds: Arc::new(Mutex::new(None)),
        user_agent: Arc::new(Mutex::new(None)),
        outbound_public_tags: Arc::new(Mutex::new(None)),
        routing_substrate: crate::slots::new_routing_substrate_slot(),
        publish_resolver: crate::slots::new_publish_resolver_slot(),
        external_event_sink_policy: crate::slots::new_external_event_sink_policy_slot(),
        kernel_clock: crate::slots::new_kernel_clock_slot(),
        gc_budget_ceiling: None,
    }
    .snapshot()
}

#[test]
fn apply_to_kernel_installs_injected_reference_classifier() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    sources_with_classifier(Some(synthetic_classifier())).apply_to_kernel(&mut kernel);

    let store = kernel.event_store_handle();
    store.insert(reaction_event(), &"wss://r/".to_string(), 1_000_000).unwrap();

    let counts = store.reference_counts(&target_id()).unwrap();
    assert_eq!(
        counts.get(TEST_BUCKET),
        1,
        "apply_to_kernel must install the injected classifier so the store counts at ingest",
    );
}

#[test]
fn apply_to_kernel_without_classifier_leaves_store_inert() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    sources_with_classifier(None).apply_to_kernel(&mut kernel);

    let store = kernel.event_store_handle();
    store.insert(reaction_event(), &"wss://r/".to_string(), 1_000_000).unwrap();

    assert!(
        store.reference_counts(&target_id()).unwrap().is_empty(),
        "no injected classifier → the store maintains no reference counts",
    );
}
