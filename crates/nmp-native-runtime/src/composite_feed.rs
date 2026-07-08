//! `NmpApp::open_composite_feed` — composition root for #3082 composite
//! multi-lane feeds (#3086 BLOCKER 1: make `open_composite_feed` reachable).
//!
//! `nmp_feed_session::open_composite_feed` (the settled composite-lane
//! compiler) needs a [`LaneMappingRegistry`] naming which protocol extractors
//! a lane may reference (`nip18.target`, `nip22.root`) alongside `nmp-feed`'s
//! own `feed.authored` identity mapping. Registering protocol mappings is a
//! composition-root responsibility (ADR-0069) — the composite-lane compiler
//! itself never learns a kind or a protocol name (D0) — so this module builds
//! ONE process-shared registry at [`NmpApp`] construction and exposes
//! [`NmpApp::open_composite_feed`] as the sibling entry point to
//! [`NmpApp::open_feed`] (`feed_session.rs`), over the SAME
//! compile → record-in-registry → hand-back-a-handle lifecycle.
//!
//! Binding-surface (Swift/Kotlin/TS UniFFI) exposure of composite feeds is a
//! separate, later pass — out of scope here. This only makes the Rust surface
//! reachable and host-tested end to end through a real [`NmpApp`].

use std::sync::Arc;

use crate::{FeedOpenError, NmpApp};
use nmp_feed::{CompositeFeedParams, FeedHandle, FeedSessionId, LaneMappingId, LaneMappingRegistry};

/// Build the process-shared composite-feed lane-mapping registry: `nmp-feed`'s
/// own `feed.authored` (pre-installed by [`LaneMappingRegistry::new`]) plus
/// the protocol-owned `nip18.target`/`nip22.root` mappings. Register-once —
/// called exactly once, at [`NmpApp`] construction (`app_ctor.rs`).
#[must_use]
pub(crate) fn composite_lane_mappings() -> LaneMappingRegistry {
    let registry = LaneMappingRegistry::new();
    registry.register(
        LaneMappingId(nmp_nip18::NIP18_TARGET_MAPPING_ID.to_string()),
        nmp_nip18::nip18_target_mapping(),
    );
    registry.register(
        LaneMappingId(nmp_nip22::NIP22_ROOT_MAPPING_ID.to_string()),
        nmp_nip22::nip22_root_mapping(),
    );
    registry
}

impl NmpApp {
    /// #3082/#3086 — open ONE composite multi-lane feed session.
    ///
    /// Mirrors [`NmpApp::open_feed`]'s lifecycle: run the canonical composite
    /// compiler (each lane resolves its acquisition scope through the SAME
    /// step-3 compiler `open_feed` uses — there is no second acquisition
    /// resolver), record the resulting teardown recipe in the SAME
    /// engine-agnostic session registry under a freshly minted id, and return
    /// a [`FeedHandle`] pairing the projection key with the session id. A
    /// failed compile registers nothing (fail-closed); a registry failure
    /// runs the just-produced teardown immediately (same contract as
    /// `open_feed`).
    pub fn open_composite_feed(
        &self,
        params: &CompositeFeedParams,
    ) -> Result<FeedHandle, FeedOpenError> {
        let mappings = Arc::clone(&self.lane_mappings);
        self.open_composite_feed_with_mappings(params, &mappings)
    }

    /// Internal/test/composition seam — see `open_composite_feed`'s doc for
    /// the canonical entry point and `testing.rs`'s
    /// `open_composite_feed_with_mappings_for_test` for why a caller would
    /// ever want a DIFFERENT registry than `self.lane_mappings`.
    pub(crate) fn open_composite_feed_with_mappings(
        &self,
        params: &CompositeFeedParams,
        mappings: &LaneMappingRegistry,
    ) -> Result<FeedHandle, FeedOpenError> {
        let build = nmp_feed_session::open_composite_feed(self, params, mappings)?;
        let projection_key = build.projection_key.clone();

        let session_id = self.feed_sessions.open(build);
        if session_id == FeedSessionId(0) {
            // Registry poisoned: `open` already ran teardown, nothing leaked.
            return Err(FeedOpenError::RegistryUnavailable);
        }

        Ok(FeedHandle {
            projection_key,
            session_id,
        })
    }
}

#[cfg(test)]
#[path = "composite_feed_tests.rs"]
mod tests;
