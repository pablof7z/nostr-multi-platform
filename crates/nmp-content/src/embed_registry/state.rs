//! Registry state — the target→entry map plus the inherent
//! claim/release/query API apps call from FFI bindings.
//!
//! Refcount integrity: each [`Entry`] tracks the set of *live* handle ids
//! for its target. `release` only decrements when the supplied
//! `handle_id` is currently live, so a double-release of one handle or a
//! phantom handle can never decrement another claim's refcount. The
//! refcount is exactly `handles.len()` — one source of truth.

use std::collections::{BTreeMap, BTreeSet};

use super::target::{ClaimHandle, EmbedTarget, ResolvedEvent};

/// In-memory entry per target.
///
/// `handles` is the set of currently-live handle ids; `refcount()` is
/// `handles.len()`. Inserts/removes happen only on the UI-driven
/// `claim`/`release` path — never per kernel event — so this stays clear
/// of the D8 hot path.
#[derive(Clone, Debug, Default)]
pub(super) struct Entry {
    /// Live handle ids for this target. Set membership is the refcount.
    pub(super) handles: BTreeSet<u64>,
    /// Resolved event payload — `None` until kernel ingest delivers it
    /// via `on_event_inserted`.
    pub(super) resolved: Option<ResolvedEvent>,
}

impl Entry {
    pub(super) fn refcount(&self) -> usize {
        self.handles.len()
    }
}

/// State held inside the actor — the map of target → entry plus a counter
/// for handle uniqueness.
///
/// `handle_seq` is a plain `u64`, not an atomic: the state lives behind the
/// actor's exclusive `&mut` and every mutation path (`claim`) already holds
/// that borrow, so a lock-free counter would only advertise a cross-thread
/// contract this type does not have.
pub struct EmbedClaimState {
    pub(super) entries: BTreeMap<EmbedTarget, Entry>,
    handle_seq: u64,
}

impl EmbedClaimState {
    pub(super) fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            handle_seq: 0,
        }
    }

    fn next_handle_id(&mut self) -> u64 {
        let id = self.handle_seq;
        self.handle_seq += 1;
        id
    }
}

/// Claim a target. Registers a fresh live handle and returns it plus the
/// current [`ResolvedEvent`] when present (cold-start → `None`; warm or
/// post-fetch → `Some`).
pub(super) fn claim(
    state: &mut EmbedClaimState,
    target: EmbedTarget,
) -> (ClaimHandle, Option<ResolvedEvent>) {
    let handle_id = state.next_handle_id();
    let entry = state.entries.entry(target.clone()).or_default();
    entry.handles.insert(handle_id);
    let resolved = entry.resolved.clone();
    (ClaimHandle { target, handle_id }, resolved)
}

/// Release a previously-claimed handle.
///
/// Returns `true` iff this call removed the *last* live handle for the
/// target (so the caller can act on the "all observers gone" signal —
/// e.g. start a grace-period timer). A double-release of the same handle
/// or a phantom handle (unknown target *or* stale handle id) is a no-op
/// returning `false`; it never decrements another claim's refcount.
pub(super) fn release(state: &mut EmbedClaimState, handle: &ClaimHandle) -> bool {
    let Some(entry) = state.entries.get_mut(&handle.target) else {
        return false;
    };
    if !entry.handles.remove(&handle.handle_id) {
        // Phantom / already-released handle id — no live refcount to touch.
        return false;
    }
    if entry.handles.is_empty() {
        state.entries.remove(&handle.target);
        true
    } else {
        false
    }
}

/// True if any handle is currently outstanding for `target`.
pub(super) fn is_claimed(state: &EmbedClaimState, target: &EmbedTarget) -> bool {
    state.entries.get(target).is_some_and(|e| e.refcount() > 0)
}

/// Current refcount for `target` (0 if absent).
pub(super) fn refcount(state: &EmbedClaimState, target: &EmbedTarget) -> usize {
    state.entries.get(target).map_or(0, Entry::refcount)
}

/// Number of distinct targets currently being claimed.
pub(super) fn claim_count(state: &EmbedClaimState) -> usize {
    state.entries.len()
}

/// Look up a resolved payload, if any.
pub(super) fn resolved(state: &EmbedClaimState, target: &EmbedTarget) -> Option<ResolvedEvent> {
    state.entries.get(target).and_then(|e| e.resolved.clone())
}
