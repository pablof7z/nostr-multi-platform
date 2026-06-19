//! Unit tests for the canonical feed surface (ADR-0061): descriptor
//! canonicalization determinism and the viewport → controller decision.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;
use crate::FeedController;

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";

fn home_descriptor() -> FeedDescriptor {
    FeedDescriptor {
        profile: "notes".into(),
        source: FeedSource::HomeFollowSet {},
        scope: FeedScope::ActiveAccount,
    }
}

fn home_json() -> String {
    serde_json::to_string(&home_descriptor()).unwrap()
}

/// A controller that counts `load_older` calls and returns a scripted sequence
/// of progress outcomes (a `true` for each available page, then `false` =
/// exhausted).
struct ScriptedController {
    calls: AtomicUsize,
    pages: usize,
}

impl ScriptedController {
    fn new(pages: usize) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            pages,
        })
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl FeedController for ScriptedController {
    fn load_older(&self) -> bool {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        n < self.pages
    }
}

/// Opener that binds every descriptor to one controller + a tiny page policy
/// (threshold 5, cap 1000).
struct StubOpener {
    controller: Arc<dyn FeedController>,
    policy: FeedPagePolicy,
}

impl FeedOpener for StubOpener {
    fn open(&self, _descriptor: &FeedDescriptor) -> Option<OpenedFeed> {
        Some(OpenedFeed {
            controller: self.controller.clone(),
            page_policy: self.policy,
        })
    }
}

// ─── Canonicalization ───────────────────────────────────────────────────────────

#[test]
fn same_descriptor_yields_same_key() {
    let a = canonical_feed_key(&home_descriptor());
    let b = canonical_feed_key(&home_descriptor());
    assert_eq!(a, b, "the same descriptor must always canonicalize equal");
    // And via the JSON open path (what the C-ABI / wasm hosts use).
    let surface = FeedSurface::default();
    let h1 = surface.open(&home_json()).expect("open");
    assert_eq!(h1.key, a, "open's key matches direct canonicalization");
}

#[test]
fn distinct_descriptors_yield_distinct_keys() {
    let home = canonical_feed_key(&home_descriptor());
    let author = canonical_feed_key(&FeedDescriptor {
        profile: "notes".into(),
        source: FeedSource::Author {
            pubkey: ALICE.to_string(),
        },
        scope: FeedScope::Global,
    });
    assert_ne!(home, author, "different sources must not collide");
}

#[test]
fn key_is_independent_of_json_field_order() {
    // Same descriptor, fields in a different textual order — the canonical key
    // must be identical (so Rust / C-ABI / wasm agree regardless of how the
    // host serialized the request).
    let reordered = r#"{"scope":"activeAccount","source":{"homeFollowSet":{}},"profile":"notes"}"#;
    let surface = FeedSurface::default();
    let from_reordered = surface.open(reordered).expect("open reordered");
    assert_eq!(from_reordered.key, canonical_feed_key(&home_descriptor()));
}

#[test]
fn malformed_descriptor_fails_closed() {
    let surface = FeedSurface::default();
    assert!(surface.open("not json").is_none(), "garbage ⇒ None");
    assert!(
        surface.open(r#"{"profile":"notes"}"#).is_none(),
        "missing fields ⇒ None (fail closed)"
    );
}

// ─── Viewport → controller decision ─────────────────────────────────────────────

fn surface_with(pages: usize, policy: FeedPagePolicy) -> (FeedSurface, Arc<ScriptedController>) {
    let surface = FeedSurface::default();
    let controller = ScriptedController::new(pages);
    surface.register_opener(Arc::new(StubOpener {
        controller: controller.clone(),
        policy,
    }));
    (surface, controller)
}

#[test]
fn viewport_within_window_does_not_drive_a_drain() {
    let policy = FeedPagePolicy {
        default_page: 20,
        cap: 1000,
        prefetch_threshold: 5,
    };
    let (surface, controller) = surface_with(3, policy);
    let key = surface.open(&home_json()).unwrap().key;
    // last_visible 2, rendered_len 100 ⇒ 95 rows below ⇒ far from tail.
    let changed = surface.set_viewport(
        key.as_str(),
        FeedViewportIntent {
            first_visible: 0,
            last_visible: 2,
            rendered_len: 100,
        },
    );
    assert!(!changed, "viewport far from tail must NOT drive a drain");
    assert_eq!(controller.calls(), 0, "controller untouched");
    assert_eq!(surface.tail_state(key.as_str()), TailState::IdleMore);
}

#[test]
fn viewport_past_threshold_drives_a_drain() {
    let policy = FeedPagePolicy {
        default_page: 20,
        cap: 1000,
        prefetch_threshold: 5,
    };
    let (surface, controller) = surface_with(3, policy);
    let key = surface.open(&home_json()).unwrap().key;
    // last_visible 18, rendered_len 20 ⇒ 1 row below ⇒ within threshold 5.
    let changed = surface.set_viewport(
        key.as_str(),
        FeedViewportIntent {
            first_visible: 10,
            last_visible: 18,
            rendered_len: 20,
        },
    );
    assert!(changed, "viewport near tail must drive a drain that progresses");
    assert_eq!(controller.calls(), 1, "exactly one drain driven");
}

#[test]
fn same_rendered_len_does_not_double_drain() {
    let policy = FeedPagePolicy {
        default_page: 20,
        cap: 1000,
        prefetch_threshold: 5,
    };
    let (surface, controller) = surface_with(5, policy);
    let key = surface.open(&home_json()).unwrap().key;
    let intent = FeedViewportIntent {
        first_visible: 10,
        last_visible: 19,
        rendered_len: 20,
    };
    assert!(surface.set_viewport(key.as_str(), intent));
    // Same rendered_len reported again (no growth) ⇒ must NOT drain again.
    assert!(!surface.set_viewport(key.as_str(), intent));
    assert_eq!(controller.calls(), 1, "duplicate-drain guard holds");
}

#[test]
fn exhaustion_stops_further_drains() {
    let policy = FeedPagePolicy {
        default_page: 20,
        cap: 1000,
        prefetch_threshold: 5,
    };
    // 0 pages ⇒ the first load_older returns false (no progress).
    let (surface, controller) = surface_with(0, policy);
    let key = surface.open(&home_json()).unwrap().key;
    let changed = surface.set_viewport(
        key.as_str(),
        FeedViewportIntent {
            first_visible: 0,
            last_visible: 19,
            rendered_len: 20,
        },
    );
    assert!(!changed, "a no-progress drain returns false");
    assert_eq!(surface.tail_state(key.as_str()), TailState::Exhausted);
    // A later, grown viewport must NOT re-drive once exhausted.
    let changed2 = surface.set_viewport(
        key.as_str(),
        FeedViewportIntent {
            first_visible: 10,
            last_visible: 39,
            rendered_len: 40,
        },
    );
    assert!(!changed2);
    assert_eq!(controller.calls(), 1, "exhausted feed is never re-drained");
}

#[test]
fn cap_blocks_auto_extend() {
    let policy = FeedPagePolicy {
        default_page: 20,
        cap: 30,
        prefetch_threshold: 5,
    };
    let (surface, controller) = surface_with(5, policy);
    let key = surface.open(&home_json()).unwrap().key;
    // rendered_len 30 == cap ⇒ within_cap false ⇒ no drain.
    let changed = surface.set_viewport(
        key.as_str(),
        FeedViewportIntent {
            first_visible: 20,
            last_visible: 29,
            rendered_len: 30,
        },
    );
    assert!(!changed, "at cap, NMP stops auto-extending");
    assert_eq!(controller.calls(), 0);
}

#[test]
fn no_opener_yields_handle_but_inert_viewport() {
    // No opener registered: open still returns a deterministic handle (ADR-0039
    // — the key is identity, not state), but viewport reports are inert.
    let surface = FeedSurface::default();
    let handle = surface.open(&home_json()).expect("handle returned");
    assert_eq!(handle.key, canonical_feed_key(&home_descriptor()));
    assert_eq!(surface.tail_state(handle.key.as_str()), TailState::Unavailable);
    let changed = surface.set_viewport(
        handle.key.as_str(),
        FeedViewportIntent {
            first_visible: 0,
            last_visible: 19,
            rendered_len: 20,
        },
    );
    assert!(!changed, "unbound feed: viewport drives nothing");
}

#[test]
fn close_forgets_the_open_feed() {
    let (surface, _c) = surface_with(3, FeedPagePolicy::default());
    let key = surface.open(&home_json()).unwrap().key;
    assert!(surface.is_open(key.as_str()));
    assert!(surface.close(key.as_str()));
    assert!(!surface.is_open(key.as_str()));
    assert!(!surface.close(key.as_str()), "double close ⇒ no-op false");
}
