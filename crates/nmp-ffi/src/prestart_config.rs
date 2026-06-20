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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nmp_core::substrate::HostOpHandler;

    use crate::{nmp_app_free, nmp_app_new, nmp_app_start, NmpConfigStatus};

    struct DummyHostOpHandler;

    impl HostOpHandler for DummyHostOpHandler {
        fn handle(&self, _action_json: &str, _correlation_id: &str) -> serde_json::Value {
            serde_json::json!({ "ok": true })
        }
    }

    #[test]
    fn app_host_setter_after_start_is_rejected_and_recorded() {
        let app = nmp_app_new();
        nmp_app_start(app, 256, 4);

        let app_ref = unsafe { &*app };
        assert_eq!(
            app_ref.set_host_op_handler(Arc::new(DummyHostOpHandler)),
            NmpConfigStatus::AlreadyStarted
        );
        let records = app_ref.composition_ledger().to_json()["records"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(
            records.iter().any(|record| {
                record["seam"] == "host_op_handler" && record["disposition"] == "DroppedLateWiring"
            }),
            "late AppHost setter should be visible in the composition ledger"
        );

        nmp_app_free(app);
    }
}
