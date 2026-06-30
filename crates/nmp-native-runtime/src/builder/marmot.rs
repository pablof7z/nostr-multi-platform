//! Feature-gated Marmot builder helpers.
//!
//! These helpers expose only the live credential-slot wrapper/config needed by
//! `nmp_marmot::install`. They do not install Marmot implicitly.

use std::path::PathBuf;

use super::NmpAppBuilder;

impl<S> NmpAppBuilder<S> {
    /// Clone the live MLS credential slot into Marmot's owner wrapper.
    ///
    /// This is available during explicit composition:
    /// build the config first, then pass the builder mutably to
    /// `nmp_marmot::install`.
    #[must_use]
    pub fn marmot_local_credential_slot(&self) -> nmp_marmot::MarmotLocalCredentialSlot {
        let app = unsafe { &*self.app };
        app.marmot_local_credential_slot()
    }

    /// Build a Marmot config from a caller-owned storage directory.
    #[must_use]
    pub fn marmot_config(&self, storage_dir: impl Into<PathBuf>) -> nmp_marmot::MarmotConfig {
        let app = unsafe { &*self.app };
        app.marmot_config(storage_dir)
    }
}
