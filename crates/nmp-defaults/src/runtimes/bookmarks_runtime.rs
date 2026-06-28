//! NIP-51 bookmark-list runtime wiring.
//!
//! This composition helper installs one shared [`BookmarkListProjection`] as
//! the kind:10003 read model and read-modify-write state backing the default
//! add/remove bookmark actions. It also owns an active-account observed
//! projection reconciler registered via the **identity-change observer** seam
//! (`register_identity_change_observer`) that opens / closes the concrete
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
    ActionRegistrar, HostCapabilities, IdentityChangeRegistrar, ObservedProjectionReconciler,
    ObservedProjectionRegistrar, SnapshotProjectionRegistrar,
};
use nmp_core::ObservedProjectionSink;
use nmp_nip51::{active_bookmark_list_interest, BookmarkListProjection};

/// Wire active-account kind:10003 bookmark projection and safe write actions,
/// and register the identity-change interest reconciler.
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
              + SnapshotProjectionRegistrar
              + IdentityChangeRegistrar),
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
    //
    // Identity-change-driven: no tick polling. The live_shape closure reads the
    // active pubkey slot directly, returning Some(shape) when signed in and
    // None on logout/reset so no stale subscription lingers.
    let observer = Arc::clone(&projection) as Arc<dyn ObservedProjectionSink>;
    let active_pubkey = app.active_pubkey();
    let reconciler = ObservedProjectionReconciler::new(
        app.observed_projection_registrar_handle(),
        observer,
        "nmp.nip51.bookmarks",
        1,
        128,
        Arc::new(move || {
            let pubkey = active_pubkey.lock().ok()?.clone()?;
            Some(active_bookmark_list_interest(&pubkey).shape)
        }),
    );
    let reconciler_for_identity = reconciler.clone();
    app.register_identity_change_observer(move |_| reconciler_for_identity.sync());
    // Eager sync for cold-start: account may already be set.
    reconciler.sync();

    projection
}

/// Wire add/remove bookmark-set-item actions (kind:30003 / kind:30004) into `app`.
///
/// Creates one [`nmp_nip51::BookmarkSetsProjection`] and registers both
/// [`nmp_nip51::AddBookmarkSetItemAction`] and
/// [`nmp_nip51::RemoveBookmarkSetItemAction`] against it.
pub fn register_bookmark_set_runtime(app: &mut (impl ActionRegistrar + HostCapabilities)) {
    let projection = Arc::new(nmp_nip51::BookmarkSetsProjection::new(app.active_pubkey()));
    nmp_nip51::register_bookmark_set_actions(app, projection);
}

/// Wire the publish-web-bookmark action (kind:39701 NIP-B0) into `app`.
///
/// Creates one [`nmp_nip51::WebBookmarksProjection`] and registers
/// [`nmp_nip51::PublishWebBookmarkAction`] against it.
pub fn register_web_bookmark_runtime(app: &mut (impl ActionRegistrar + HostCapabilities)) {
    let projection = Arc::new(nmp_nip51::WebBookmarksProjection::new(app.active_pubkey()));
    nmp_nip51::register_web_bookmark_actions(app, projection);
}

// Co-located bookmark active observed-projection reconciler tests live in a
// sibling file to hold this module under the 300-LOC ceiling.
#[cfg(test)]
#[path = "runtimes_bookmarks_tests.rs"]
mod bookmarks_tests;
