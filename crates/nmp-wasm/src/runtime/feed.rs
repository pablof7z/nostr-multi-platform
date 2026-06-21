//! App-facing feed declaration helpers for [`super::WasmRuntime`].
//!
//! The public surface accepts primary content kinds. Protocol wrapper
//! acquisition is derived here, below app composition and above the pure
//! reducer, so apps do not compile repost shapes themselves.

use super::WasmRuntime;

impl WasmRuntime {
    /// Declare an active-follows feed from app-owned primary content kinds.
    ///
    /// This is the wasm twin of `NmpApp::declare_active_follows_feed`.
    /// Callers name only the primary kinds they intend to render. NIP-18
    /// repost wrappers are derived here before the pure reducer receives the
    /// compiled acquisition set, so app composition never has to say
    /// "kind 1 plus kind 6" or "kind 20 plus kind 16".
    ///
    /// Returns `false` when a caller supplies a wrapper kind as primary input
    /// or otherwise fails NIP-18 primary-kind validation. The reducer is left
    /// unchanged on failure.
    pub fn declare_active_follows_feed<I>(&self, primary_kinds: I) -> bool
    where
        I: IntoIterator<Item = u32>,
    {
        let Ok(acquisition_kinds) = nmp_nip18::try_acquisition_kinds_for_primary(primary_kinds)
        else {
            return false;
        };
        let outbound = self
            .reducer
            .borrow_mut()
            .declare_active_follows_feed(acquisition_kinds);
        self.fan_outbound(outbound);
        true
    }

    /// Clear the active-follows feed declaration.
    pub fn clear_active_follows_feed(&self) {
        let outbound = self.reducer.borrow_mut().clear_active_follows_feed();
        self.fan_outbound(outbound);
    }
}
