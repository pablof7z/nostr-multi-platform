use std::collections::BTreeSet;

use nmp_core::slots::ActiveAccountSlot;
use nmp_nip02::ActiveFollowSet;
use nmp_planner::InterestShape;

/// Build the LIVE active-follows pull [`InterestShape`], or `None` to fail closed.
///
/// B1 — race-free fail-close. The active-account slot is read **first**: on
/// logout / account-switch the actor can null the slot BEFORE the async identity
/// observer clears [`ActiveFollowSet`] through the runtime update listener, so a synchronous
/// `load_older` can observe `slot == None` while `follow_set.follows()` is still
/// stale. Reading the slot first means no live active account ⇒ `None` ⇒ no
/// shape ⇒ no pull (never a stale-viewer pull, never a broad-scan; D5).
pub(super) fn live_active_follows_shape(
    account_slot: &ActiveAccountSlot,
    follow_set: &ActiveFollowSet,
    kinds: &BTreeSet<u32>,
) -> Option<InterestShape> {
    if kinds.is_empty() {
        return None;
    }
    let viewer = read_active(account_slot)?;
    let mut authors: BTreeSet<String> = follow_set.follows().into_iter().collect();
    authors.insert(viewer);
    Some(InterestShape::timeline_for(authors, kinds.clone()))
}

/// Read the active account's hex pubkey from the slot, or `None` when no
/// account is signed in or the lock is poisoned (D6).
pub(crate) fn read_active(slot: &ActiveAccountSlot) -> Option<String> {
    match slot.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => None,
    }
}
