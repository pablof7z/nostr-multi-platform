//! Interaction-counter helpers for the Mem backend (issue #1519).
//!
//! Extracted from `insert.rs` to keep that file under the 500-LOC hard cap.
//! `ic_increment` and `ic_decrement` are the only entry points; they are
//! `pub(super)` so `insert.rs`, `gc.rs`, and `store_impl.rs` can all call
//! them without promoting them to the crate public API.

use super::MemState;

/// Increment the interaction counter for the target event identified by
/// `kind` + `tags`. No-op for non-counter kinds (classify returns None).
pub(super) fn ic_increment(st: &mut MemState, kind: u32, tags: &[Vec<String>]) {
    let Some((ck, target_hex)) = crate::interaction::classify(kind, tags) else {
        return;
    };
    let count = st.interaction_counters.entry((target_hex, ck as u8)).or_insert(0);
    *count = count.saturating_add(1);
}

/// Decrement the interaction counter for the target event identified by
/// `kind` + `tags`. Removes the entry when it reaches 0 (no zero-valued rows stored).
pub(super) fn ic_decrement(st: &mut MemState, kind: u32, tags: &[Vec<String>]) {
    let Some((ck, target_hex)) = crate::interaction::classify(kind, tags) else {
        return;
    };
    let key = (target_hex, ck as u8);
    if let Some(count) = st.interaction_counters.get_mut(&key) {
        if *count <= 1 {
            st.interaction_counters.remove(&key);
        } else {
            *count -= 1;
        }
    }
}
