//! Small shared helpers for the Chirp FFI surface: a null-aware C-string
//! reader for the bespoke Chirp registration entrypoints.
//!
//! The typed action-body POD structs (`ReactAction`, `PubkeyAction`) that
//! used to live here moved to `crates/nmp-nip02/src/lib.rs` together with
//! the `Chirp{React,Follow,Unfollow}Module` impls — see `super::actions`
//! for the registration shim.

use std::ffi::{c_char, CStr};

pub(super) fn c_string_opt(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees `ptr` (when non-null) is a valid
    // nul-terminated C string for the duration of this call.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(std::borrow::ToOwned::to_owned)
}

use std::num::NonZeroUsize;

use nmp_planner::InterestShape;
use nmp_store::{PullPage, ScanLogResult};
use nmp_core::{pull_page_over, PullLimits, PullScope};

/// Build the `InterestShape` for an author flat feed (kind policy from caller).
#[must_use]
pub(super) fn author_feed_shape(pubkey_hex: &str, kinds: &[u32]) -> Option<InterestShape> {
    let k = kinds.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(",");
    InterestShape::from_filter_json(&format!(r#"{{"kinds":[{k}],"authors":["{pubkey_hex}"]}}"#))
}

/// Build the `InterestShape` for the reply tail of a thread feed (the `#e`
/// covered shape). The root-by-id half is event-id-only (uncovered) and must
/// be seeded separately.
#[must_use]
pub(super) fn thread_feed_shape(root_id_hex: &str, kinds: &[u32]) -> Option<InterestShape> {
    let k = kinds.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(",");
    InterestShape::from_filter_json(&format!(r##"{{"kinds":[{k}],"#e":["{root_id_hex}"]}}"##))
}

/// Build a [`nmp_feed::PullFn`] for a [`nmp_feed::PullFeedController`]. Captures
/// the store slot handle so a `Reset` is observed without re-capturing. Returns
/// an empty exhausted page when the store is unavailable (pre-start), causing
/// the drain to terminate cleanly and retry on the next `load_older`.
pub(super) fn make_pull_fn(store_handle: nmp_core::slots::EventStoreSlot) -> nmp_feed::PullFn {
    let limits = PullLimits {
        max_entries: NonZeroUsize::new(nmp_feed::DEFAULT_PULL_PAGE_SIZE)
            .unwrap_or(NonZeroUsize::MIN),
        max_scan_entries: NonZeroUsize::new(nmp_feed::DEFAULT_PULL_SCAN_BUDGET)
            .unwrap_or(NonZeroUsize::MIN),
    };
    std::sync::Arc::new(move |scope: PullScope, after_seq: u64| {
        let store = match store_handle.lock().ok().and_then(|g| g.clone()) {
            Some(s) => s,
            None => return empty_page(after_seq),
        };
        match pull_page_over(store.as_ref(), scope, after_seq, limits) {
            Ok(r) => r,
            Err(_) => empty_page(after_seq),
        }
    })
}

fn empty_page(at_seq: u64) -> ScanLogResult {
    ScanLogResult::Page(PullPage {
        entries: vec![],
        next_after_seq: at_seq,
        latest_seq: at_seq,
        has_more: false,
    })
}
