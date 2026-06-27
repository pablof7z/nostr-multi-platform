//! ADR-0063 (#1671 Lane F) — host-side `refs.profile` / `refs.event`
//! consumption helpers for the Rust shells (chirp-tui / chirp-desktop).
//!
//! The kernel emits `refs.profile` as a per-KEY row-delta projection: each tick's
//! sidecar payload is an NRRD [`RefRowDeltaBatch`](crate::refs::RefRowDeltaBatch)
//! carrying only the changed/cleared rows (or a full baseline on identity change /
//! first attach). A consumer that wants `profile(pubkey)` therefore CANNOT decode
//! one frame in isolation — it must maintain the stateful per-key cache the
//! row-deltas merge into. [`RefRowCache`] is that canonical merge engine; this
//! wrapper specialises it to the `"profile"` namespace and decodes each cached
//! row payload (a `KPRF` `ProfileCard` buffer) into a [`ProfileCardModel`].
//!
//! This is the ONLY app-side mirror of hydrated profile facts (D4 / invariant v):
//! the shells hold one [`RefProfileStore`] (the [`RefRowCache`] mirror), never a
//! second native `HashMap<pubkey, ProfileCard>` of their own.

use std::collections::BTreeMap;

use super::{decode_ref_row_delta_batch, RefRowApplyOutcome, RefRowCache};
use crate::kernel::public_typed_projections::{
    decode_claimed_events, decode_profile, ClaimedEventRow, ProfileCardModel,
};

/// The kernel-emitted projection key + Lane A namespace token for the profile
/// resolver. The sidecar entry is keyed by `refs.profile`; the NRRD batch inside
/// carries the bare `"profile"` namespace token.
pub const REFS_PROFILE_KEY: &str = "refs.profile";
const REFS_PROFILE_NAMESPACE: &str = "profile";
/// The kernel-emitted projection key + Lane A namespace token for the event
/// resolver. Row payloads are single-entry `KCEV` `ClaimedEventsModel` buffers.
pub const REFS_EVENT_KEY: &str = "refs.event";
const REFS_EVENT_NAMESPACE: &str = "event";

fn decode_event_row(key: &str, payload: &[u8]) -> Option<ClaimedEventRow> {
    let model = decode_claimed_events(payload).ok()?;
    let mut entries = model.entries.into_iter();
    let (entry_key, row) = entries.next()?;
    if entries.next().is_some() || entry_key != key || row.primary_id != key {
        return None;
    }
    Some(row)
}

/// Host-side consumer of the `refs.profile` row-delta projection.
///
/// Holds the persistent [`RefRowCache`] the per-key deltas merge into and exposes
/// a `profile(pubkey)` typed lookup. One instance lives for the lifetime of the
/// shell's update loop (NOT rebuilt per frame — the cache is stateful).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RefProfileStore {
    cache: RefRowCache,
}

/// Host-side consumer of the `refs.event` row-delta projection.
///
/// Holds the persistent [`RefRowCache`] the per-key event deltas merge into and
/// exposes typed event lookups by `primary_id`. Row payloads decode as a
/// single-entry [`ClaimedEventRow`] payload; malformed or mismatched rows are
/// rejected before commit (D6).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RefEventStore {
    cache: RefRowCache,
}

impl RefEventStore {
    /// Fresh empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply one frame's `refs.event` sidecar payload under the frame identity.
    pub fn apply_sidecar(
        &mut self,
        payload: &[u8],
        session_id: u64,
        snapshot_epoch: u64,
    ) -> RefRowApplyOutcome {
        let Ok(batch) = decode_ref_row_delta_batch(payload) else {
            return RefRowApplyOutcome::default();
        };
        let decode_ok = |key: &str, payload: &[u8]| decode_event_row(key, payload).is_some();
        self.cache
            .apply(&batch, session_id, snapshot_epoch, &decode_ok)
    }

    /// The decoded event row for `primary_id`, or `None` if no live ref is cached.
    #[must_use]
    pub fn event(&self, primary_id: &str) -> Option<ClaimedEventRow> {
        let payload = self.cache.get(REFS_EVENT_NAMESPACE, primary_id)?;
        decode_event_row(primary_id, &payload)
    }

    /// The full materialised `primary_id -> ClaimedEventRow` set currently cached.
    #[must_use]
    pub fn events(&self) -> BTreeMap<String, ClaimedEventRow> {
        self.cache
            .snapshot(REFS_EVENT_NAMESPACE)
            .into_iter()
            .filter_map(|(key, payload)| decode_event_row(&key, &payload).map(|row| (key, row)))
            .collect()
    }

    /// Whether the underlying cache has applied a baseline (UI-gating flag).
    #[must_use]
    pub fn baselined(&self) -> bool {
        self.cache.baselined()
    }
}

impl RefProfileStore {
    /// Fresh empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply one frame's `refs.profile` sidecar payload (an encoded NRRD batch)
    /// under the frame's `(session_id, snapshot_epoch)` identity.
    ///
    /// A malformed sidecar payload (not a valid NRRD batch) is a fail-closed
    /// no-op: the prior cache is retained and an empty outcome is returned (D6).
    /// Decode-before-commit of each ROW payload (the `KPRF` buffer) is enforced
    /// by the [`RefRowCache`] merge via the `decode_ok` preflight wired here.
    pub fn apply_sidecar(
        &mut self,
        payload: &[u8],
        session_id: u64,
        snapshot_epoch: u64,
    ) -> RefRowApplyOutcome {
        let Ok(batch) = decode_ref_row_delta_batch(payload) else {
            // Fail closed (D6): a garbage sidecar never empties or corrupts the
            // live cache — retain prior state, signal "nothing changed".
            return RefRowApplyOutcome::default();
        };
        // Decode-before-commit: a row commits only if its KPRF payload decodes to
        // a ProfileCard. A malformed row leaves the prior cached row intact and
        // latches resync inside the cache (invariant #2).
        let decode_ok = |_key: &str, payload: &[u8]| decode_profile(payload).is_ok();
        self.cache
            .apply(&batch, session_id, snapshot_epoch, &decode_ok)
    }

    /// The decoded [`ProfileCardModel`] for `pubkey`, or `None` if no live ref is
    /// cached for that key. Reads the kernel-pushed typed row directly — there is
    /// no second app-side cache (D4 / invariant v).
    #[must_use]
    pub fn profile(&self, pubkey: &str) -> Option<ProfileCardModel> {
        let payload = self.cache.get(REFS_PROFILE_NAMESPACE, pubkey)?;
        decode_profile(&payload).ok()
    }

    /// The full materialised `pubkey -> ProfileCardModel` set currently cached.
    /// Convenience for shells that render against a map (e.g. desktop's
    /// `feed_card` / `mention_label`). Rows whose payload fails to decode are
    /// skipped (they cannot be in the cache: decode-before-commit gates entry).
    #[must_use]
    pub fn profiles(&self) -> BTreeMap<String, ProfileCardModel> {
        self.cache
            .snapshot(REFS_PROFILE_NAMESPACE)
            .into_iter()
            .filter_map(|(key, payload)| decode_profile(&payload).ok().map(|card| (key, card)))
            .collect()
    }

    /// Whether the underlying cache has applied a baseline (UI-gating flag).
    #[must_use]
    pub fn baselined(&self) -> bool {
        self.cache.baselined()
    }
}

#[cfg(test)]
#[path = "host_store_tests.rs"]
mod tests;
