//! Generic reference-counter helpers for the Mem backend (#2512, was #1519).
//!
//! Extracted from `insert.rs` to keep that file under the 500-LOC hard cap.
//! `ic_increment` and `ic_decrement` are the only entry points; they are
//! `pub(super)` so `insert.rs`, `gc.rs`, and `store_impl.rs` can all call
//! them without promoting them to the crate public API.
//!
//! Classification is NOT done here: it is the installed `reference_classifier`
//! (injected by `nmp-relations`). No classifier installed → both helpers no-op.

use super::MemState;

/// Increment the reference counter for the bucket + target the installed
/// classifier picks from `kind` + `tags`. No-op when no classifier is installed
/// or the event is not a counted reference.
pub(super) fn ic_increment(st: &mut MemState, kind: u32, tags: &[Vec<String>]) {
    let Some((bucket, target_hex)) = classify(st, kind, tags) else {
        return;
    };
    let count = st.interaction_counters.entry((target_hex, bucket)).or_insert(0);
    *count = count.saturating_add(1);
}

/// Decrement the reference counter for the classified bucket + target. Removes
/// the entry when it reaches 0 (no zero-valued rows stored). No-op when no
/// classifier is installed or the event is not a counted reference.
pub(super) fn ic_decrement(st: &mut MemState, kind: u32, tags: &[Vec<String>]) {
    let Some((bucket, target_hex)) = classify(st, kind, tags) else {
        return;
    };
    let key = (target_hex, bucket);
    if let Some(count) = st.interaction_counters.get_mut(&key) {
        if *count <= 1 {
            st.interaction_counters.remove(&key);
        } else {
            *count -= 1;
        }
    }
}

/// Run the installed classifier, returning `(bucket_discriminant, target_hex)`.
/// Clones the `Arc` so the `&MemState` borrow does not overlap the later
/// `&mut` access to `interaction_counters`.
fn classify(st: &MemState, kind: u32, tags: &[Vec<String>]) -> Option<(u8, String)> {
    let classify = st.reference_classifier.clone()?;
    classify(kind, tags).map(|(bucket, target)| (bucket.discriminant(), target))
}
