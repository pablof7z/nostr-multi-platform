//! Native capability-slot accessors.

use std::sync::Arc;

use nmp_core::__ffi_internal::CapabilityCallbackSlot;

use crate::NmpApp;

impl NmpApp {
    /// Arc clone of the per-app capability callback slot.
    #[doc(hidden)]
    #[must_use]
    pub fn capability_callback_slot(&self) -> CapabilityCallbackSlot {
        Arc::clone(&self.capability_callback)
    }
}
