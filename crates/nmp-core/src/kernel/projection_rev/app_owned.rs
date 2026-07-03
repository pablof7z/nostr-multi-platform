//! NMP#2944 — content-advancing `projection_rev` for app-owned projection keys.
//!
//! The kernel's `ProjectionRevTracker` derives per-key revs from the
//! `BUILTIN_PROJECTION_DEPENDENCIES` table, which only covers Tier-2 built-in
//! keys. Host-registered (app-owned) keys — e.g. an app's `chirp.timeline.home`
//! OP-feed projection — are absent from that table, so `stamp_typed_projections`
//! leaves their `projection_rev` at the default 0 on every tick, even when the
//! payload changes (0 cards → N cards).
//!
//! That violates the `projection_rev` wire contract (ADR-0070 Rung 2): the rev
//! is documented as monotonic and advancing when the key's content changes, and
//! the rev-aware host apply caches (the generated iOS
//! `ProjectionCache.generated.swift` and the Android `ProjectionCache.kt`) rely
//! on it: they skip a `Changed` row when `incomingRev <= cached.rev`. A rev
//! frozen at 0 across a content change means the host commits the FIRST (often
//! empty) payload and then skips every later one — the home feed renders empty
//! forever while the frame-level envelope rev keeps advancing (the observed
//! "decodedRev advanced but no card" device symptom, NMP#2944).
//!
//! This module derives a content-driven rev for app-owned keys: a per-key
//! counter that increments whenever the payload fingerprint changes, so the rev
//! advances iff the content changed — exactly the contract. `Cleared` rows keep
//! rev 0 (the host removes the key regardless of rev).

use std::hash::{Hash, Hasher};

use super::ProjectionManifest;
use super::ProjectionRevTracker;
use crate::update_envelope::{TypedProjectionData, WireProjectionState};

fn payload_fingerprint(payload: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    payload.hash(&mut h);
    h.finish()
}

impl ProjectionRevTracker {
    /// Stamp app-owned (non-manifest) `Changed` entries with a content-advancing
    /// `projection_rev`. Manifest-tracked (Tier-2 built-in) entries are left
    /// untouched — their rev was already stamped from the manifest. `Cleared`
    /// rows keep rev 0. Idempotent within a tick: unchanged payloads keep their
    /// prior rev, so rev-aware hosts correctly skip re-decoding them.
    #[must_use]
    pub(crate) fn stamp_app_owned_revs(
        &mut self,
        typed: Vec<TypedProjectionData>,
        manifest: &ProjectionManifest,
    ) -> Vec<TypedProjectionData> {
        typed
            .into_iter()
            .map(|mut entry| {
                let in_manifest = manifest
                    .states
                    .iter()
                    .any(|s| s.key == entry.key.as_str());
                if in_manifest || entry.state == WireProjectionState::Cleared {
                    return entry;
                }
                let fp = payload_fingerprint(&entry.payload);
                let slot = self
                    .app_owned_revs
                    .entry(entry.key.clone())
                    .or_insert((0, 0));
                let (rev, last_fp) = *slot;
                if rev == 0 || last_fp != fp {
                    *slot = (rev + 1, fp);
                }
                entry.projection_rev = slot.0;
                entry
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, payload: &[u8], state: WireProjectionState) -> TypedProjectionData {
        TypedProjectionData {
            key: key.to_string(),
            state,
            payload: payload.to_vec(),
            ..Default::default()
        }
    }

    fn empty_manifest() -> ProjectionManifest {
        ProjectionManifest {
            session_id: 0,
            epoch: 0,
            states: Vec::new(),
        }
    }

    fn stamp_one(
        tracker: &mut ProjectionRevTracker,
        key: &str,
        payload: &[u8],
        state: WireProjectionState,
    ) -> u64 {
        tracker
            .stamp_app_owned_revs(vec![entry(key, payload, state)], &empty_manifest())[0]
            .projection_rev
    }

    #[test]
    fn app_owned_rev_advances_on_content_change_and_is_stable_otherwise() {
        let mut tracker = ProjectionRevTracker::default();
        let key = "chirp.timeline.home";

        // First emission (empty payload): 0 -> 1 so a rev-aware host commits it.
        assert_eq!(stamp_one(&mut tracker, key, b"empty", WireProjectionState::Changed), 1);
        // Same content: rev stays 1 (host correctly skips re-decode).
        assert_eq!(stamp_one(&mut tracker, key, b"empty", WireProjectionState::Changed), 1);
        // Content changes (cards arrive): rev advances so the host admits it.
        assert_eq!(stamp_one(&mut tracker, key, b"cards", WireProjectionState::Changed), 2);
        assert_eq!(stamp_one(&mut tracker, key, b"cards", WireProjectionState::Changed), 2);
        // Another change: keeps advancing (monotonic).
        assert_eq!(stamp_one(&mut tracker, key, b"more", WireProjectionState::Changed), 3);
    }

    #[test]
    fn cleared_app_owned_rows_keep_rev_zero() {
        let mut tracker = ProjectionRevTracker::default();
        assert_eq!(
            stamp_one(&mut tracker, "chirp.timeline.home", b"", WireProjectionState::Cleared),
            0
        );
    }

    #[test]
    fn distinct_app_owned_keys_track_independent_revs() {
        let mut tracker = ProjectionRevTracker::default();
        assert_eq!(stamp_one(&mut tracker, "chirp.timeline.home", b"a", WireProjectionState::Changed), 1);
        assert_eq!(stamp_one(&mut tracker, "chirp.timeline.tag.x", b"a", WireProjectionState::Changed), 1);
        assert_eq!(stamp_one(&mut tracker, "chirp.timeline.home", b"b", WireProjectionState::Changed), 2);
        // The tag feed's rev is unaffected by the home feed's change.
        assert_eq!(stamp_one(&mut tracker, "chirp.timeline.tag.x", b"a", WireProjectionState::Changed), 1);
    }
}
