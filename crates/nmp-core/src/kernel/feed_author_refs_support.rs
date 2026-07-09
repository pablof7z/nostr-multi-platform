//! Shared test-support helpers for the `feed_author_refs` test modules.
//!
//! Split out (#3116) when the #3116 equivalence test pushed
//! `feed_author_refs_tests.rs` over the file-size hard cap: the new test
//! moved to its own file (`feed_author_refs_tests_equivalence.rs`) and both
//! it and the original behavioral-spec tests share the fixtures here.

use std::sync::{Arc, Mutex};

use super::super::snapshot_registry::new_snapshot_projection_slot;
use super::super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use nmp_network::role::RelayRole;

pub(super) const HOME_KEY: &str = "test.feed.home";

pub(super) fn hex64(prefix: &str) -> String {
    format!("{prefix:0<64}").chars().take(64).collect()
}

/// A kernel with a snapshot slot bound, plus a connected relay so resolves
/// register a fetch interest (not just a cache-serve).
pub(super) fn kernel_with_slot() -> (
    Kernel,
    super::super::snapshot_registry::SnapshotProjectionSlot,
) {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connected(RelayRole::Content);
    let slot = new_snapshot_projection_slot();
    kernel.set_snapshot_projection_handle(Arc::clone(&slot));
    (kernel, slot)
}

/// Register a feed-author provider whose returned set is swappable at test time
/// via the shared `Arc<Mutex<Vec<String>>>` handle.
pub(super) fn register_swappable_provider(
    slot: &super::super::snapshot_registry::SnapshotProjectionSlot,
    feed_key: &str,
) -> Arc<Mutex<Vec<String>>> {
    let authors = Arc::new(Mutex::new(Vec::<String>::new()));
    let authors_for_closure = Arc::clone(&authors);
    slot.lock()
        .expect("registry lock")
        .register_feed_author_provider(feed_key, move || {
            authors_for_closure.lock().expect("authors lock").clone()
        });
    authors
}

pub(super) fn set_authors(handle: &Arc<Mutex<Vec<String>>>, keys: &[String]) {
    *handle.lock().expect("authors lock") = keys.to_vec();
}
