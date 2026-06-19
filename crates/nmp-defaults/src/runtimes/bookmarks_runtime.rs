//! NIP-51 bookmark-list runtime wiring.
//!
//! This composition helper installs one shared [`BookmarkListProjection`] as
//! the kind:10003 observer and read-modify-write state backing the default
//! add/remove bookmark actions.

use std::sync::Arc;

use nmp_core::substrate::{ActionRegistrar, EventObserverRegistrar, HostCapabilities};
use nmp_core::KernelEventObserver;
use nmp_nip51::BookmarkListProjection;

/// Wire active-account kind:10003 bookmark projection and safe write actions.
pub fn register_bookmark_runtime(
    app: &mut (impl ActionRegistrar + EventObserverRegistrar + HostCapabilities),
) -> Arc<BookmarkListProjection> {
    let projection = Arc::new(BookmarkListProjection::new(app.active_pubkey()));

    app.register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);

    nmp_nip51::register_bookmark_actions(app, Arc::clone(&projection));
    projection
}
