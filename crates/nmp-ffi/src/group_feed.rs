//! C-side opaque handles for native-runtime NIP-29 read-view tokens.

use crate::NmpApp;
use nmp_native_runtime::{Nip29GroupDiscoveryHandle, Nip29GroupDiscoverySession};

/// Opaque C handle for one host-driven NIP-29 read view.
///
/// Native-runtime owns the hydrating read-view session and returns a safe
/// [`Nip29GroupDiscoveryHandle`]. The C ABI handle stores the owning app address only so
/// legacy C close functions can remain handle-only.
pub struct GroupFeedHandle {
    app_addr: usize,
    handle: Nip29GroupDiscoveryHandle,
}

impl GroupFeedHandle {
    #[must_use]
    pub fn new(app: &NmpApp, handle: Nip29GroupDiscoveryHandle) -> Self {
        Self {
            app_addr: (app as *const NmpApp) as usize,
            handle,
        }
    }

    /// Tear down the view this handle owns. Consumes the handle.
    ///
    /// # Safety
    /// The `NmpApp` used at open time must still be alive.
    pub unsafe fn close(self) {
        // SAFETY: caller upholds that the app outlives the handle.
        let app = unsafe { &*(self.app_addr as *const NmpApp) };
        app.close_nip29_group_discovery_session(self.handle);
    }
}

#[must_use]
pub fn open_group_discovery_handle(app: &NmpApp, host_relay_url: String) -> GroupFeedHandle {
    GroupFeedHandle::new(
        app,
        app.open_nip29_group_discovery_session(Nip29GroupDiscoverySession::new(host_relay_url)),
    )
}
