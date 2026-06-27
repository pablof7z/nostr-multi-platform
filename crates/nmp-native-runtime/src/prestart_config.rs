//! Shared status/guard for host-init configuration that must happen before start.

use std::sync::atomic::Ordering;

use crate::NmpApp;

/// Return code for host-init configuration calls.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NmpConfigStatus {
    Ok = 0,
    NullApp = 1,
    AlreadyStarted = 2,
    Unavailable = 3,
}

impl NmpConfigStatus {
    #[must_use]
    pub const fn code(self) -> u32 {
        self as u32
    }
}

impl NmpApp {
    pub(crate) fn ensure_prestart_config(
        &self,
        seam: &'static str,
        key: impl Into<String>,
        provider: impl Into<String>,
    ) -> Result<(), NmpConfigStatus> {
        if self.started.load(Ordering::SeqCst) {
            self.composition_ledger.record(
                seam,
                key,
                provider,
                nmp_core::Disposition::DroppedLateWiring,
                None,
            );
            Err(NmpConfigStatus::AlreadyStarted)
        } else {
            Ok(())
        }
    }
}
