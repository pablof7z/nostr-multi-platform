use crate::{NmpApp, NmpConfigStatus};

impl NmpApp {
    /// Install the external event sink policy factory.
    ///
    /// Policies returned by this factory receive typed [`SignedEventFrame`]s
    /// from the `ExternalEventSinkDispatcher` on a dedicated worker thread.
    pub fn set_external_event_sink_policy_factory<F>(&self, factory: F) -> NmpConfigStatus
    where
        F: Fn(
                nmp_core::substrate::RawEventForwardPolicyContext,
            ) -> Vec<std::sync::Arc<dyn nmp_core::substrate::ExternalEventSinkPolicy>>
            + Send
            + Sync
            + 'static,
    {
        if let Err(status) = self.ensure_prestart_config(
            "external_event_sink_policy",
            "external_event_sink_policy",
            "external_event_sink_policy",
        ) {
            return status;
        }
        if let Ok(mut slot) = self.composition.external_event_sink_policy.lock() {
            self.record_slot_decision(
                "external_event_sink_policy",
                "external_event_sink_policy",
                slot.is_some(),
            );
            *slot = Some(std::sync::Arc::new(factory));
            NmpConfigStatus::Ok
        } else {
            NmpConfigStatus::Unavailable
        }
    }
}
