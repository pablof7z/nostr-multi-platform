//! Feature-gated Marmot credential-slot handoff.
//!
//! This module does not install Marmot and does not read raw key material.
//! It only wraps the actor-owned MLS nsec slot in the `nmp-marmot` owner type
//! so an explicit app composition root can call `nmp_marmot::install`.

use std::path::PathBuf;
use std::sync::Arc;

use crate::NmpApp;

impl NmpApp {
    /// Clone the live MLS credential slot into Marmot's owner wrapper.
    ///
    /// Only `nmp-marmot` reads the slot. Native runtime remains a transport
    /// owner for the shared handle and never parses or inspects the nsec.
    #[must_use]
    pub fn marmot_local_credential_slot(&self) -> nmp_marmot::MarmotLocalCredentialSlot {
        nmp_marmot::MarmotLocalCredentialSlot::new(Arc::clone(&self.read_handles.mls_local_nsec))
    }

    /// Build a Marmot config from a caller-owned storage directory.
    ///
    /// The app root still decides whether Marmot is installed by passing the
    /// returned config to `nmp_marmot::install`.
    #[must_use]
    pub fn marmot_config(&self, storage_dir: impl Into<PathBuf>) -> nmp_marmot::MarmotConfig {
        nmp_marmot::MarmotConfig::new(storage_dir, self.marmot_local_credential_slot())
    }
}
