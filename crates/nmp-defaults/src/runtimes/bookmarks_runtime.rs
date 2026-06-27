//! NIP-51 bookmark-list runtime wiring.
//!
//! This composition helper installs one shared [`BookmarkListProjection`] as
//! the kind:10003 read model and read-modify-write state backing the default
//! add/remove bookmark actions. It also owns an active-account observed
//! projection reconciler registered via the generic **per-tick observer** seam
//! (`register_snapshot_tick_observer`) that opens / closes the concrete
//! kind:10003 `authors=[pubkey]` observed projection on sign-in / account
//! switch / sign-out.
//!
//! # Ordering contract
//!
//! The observed projection is not opened until the active pubkey is known.
//! Opening uses the ADR-0062 muted-observer path, so the cache replay reaches
//! the projection before live activation and never relies on a broad kind-only
//! observer that filters by author after the fact.

use std::sync::Arc;

use nmp_core::substrate::{
    ActionRegistrar, HostCapabilities, ObservedProjectionRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::ObservedProjectionSink;
use nmp_nip51::{active_bookmark_list_interest, BookmarkListProjection};

use super::active_observed_projection::ActiveObservedProjection;

/// Wire active-account kind:10003 bookmark projection and safe write actions,
/// and register the per-tick interest reconciler.
///
/// 1. Creates one [`BookmarkListProjection`] shared across the observer and the
///    action modules.
/// 2. Registers an active-account observed-projection reconciler that opens the
///    projection only for `authors=[active]`.
/// 3. Registers the add/remove bookmark action modules (read-modify-write
///    against the same projection).
pub fn register_bookmark_runtime(
    app: &mut (impl ActionRegistrar
              + ObservedProjectionRegistrar
              + HostCapabilities
              + SnapshotProjectionRegistrar),
) -> Arc<BookmarkListProjection> {
    // ── 1. Projection ────────────────────────────────────────────────────
    let projection = Arc::new(BookmarkListProjection::new(app.active_pubkey()));

    // ── 2. Action modules ─────────────────────────────────────────────────
    nmp_nip51::register_bookmark_actions(app, Arc::clone(&projection));

    // ── 3. Snapshot projection (typed sidecar) ────────────────────────────
    let projection_for_typed = Arc::clone(&projection);
    app.register_typed_snapshot_projection("nmp.nip51.bookmarks", move || {
        let snapshot = projection_for_typed.snapshot();
        Some(nmp_core::TypedProjectionData {
            key: "nmp.nip51.bookmarks".to_string(),
            schema_id: nmp_nip51::BOOKMARK_LIST_SCHEMA_ID.to_string(),
            schema_version: nmp_nip51::BOOKMARK_LIST_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(nmp_nip51::BOOKMARK_LIST_FILE_IDENTIFIER)
                .into_owned(),
            payload: nmp_nip51::encode_bookmark_list(&snapshot),
            ..Default::default()
        })
    });

    // ── 4. Active observed-projection reconciler ─────────────────────────
    let observer = Arc::clone(&projection) as Arc<dyn ObservedProjectionSink>;
    let controller = Arc::new(ActiveObservedProjection::new(
        app.active_pubkey(),
        app.observed_projection_registrar_handle(),
        observer,
        "nmp.nip51.bookmarks",
        1,
        128,
        Arc::new(|pubkey| active_bookmark_list_interest(pubkey).shape),
    ));
    let controller_tick = Arc::clone(&controller);
    app.register_snapshot_tick_observer(move || controller_tick.sync());

    projection
}

// Co-located bookmark active observed-projection reconciler tests live in a
// sibling file to hold this module under the 300-LOC ceiling.
#[cfg(test)]
#[path = "runtimes_bookmarks_tests.rs"]
mod bookmarks_tests;
