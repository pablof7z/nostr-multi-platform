//! Cloneable observed-projection registrar handle.
//!
//! Feed-session reset hooks are `'static`, so they cannot borrow `NmpApp`.
//! This handle carries only the registry slots and actor sender needed to open
//! and close declared observed projections through the same semantics as
//! `NmpApp::open_observed_projection`.

use std::sync::Arc;

use nmp_core::substrate::ObservedProjectionCommandHandle;

use crate::app_struct::NmpApp;

pub type ObservedProjectionHandle = ObservedProjectionCommandHandle;

impl NmpApp {
    #[must_use]
    pub fn observed_projection_handle(&self) -> ObservedProjectionHandle {
        ObservedProjectionCommandHandle::new(
            Arc::clone(&self.event_observers),
            Arc::clone(&self.observed_projection_sessions),
            self.tx.clone(),
        )
    }
}
