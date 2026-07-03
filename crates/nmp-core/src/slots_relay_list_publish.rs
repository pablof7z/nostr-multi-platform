use std::sync::{Arc, Mutex};

/// Router-owned relay-list publish support installed by substrate composition.
///
/// `nmp-core` owns the account/relay-edit lifecycle that decides *when* a
/// publish is needed, but the protocol/routing crate owns the event shape and
/// cold-start target policy. The default implementation is intentionally
/// conservative: it builds no relay-list event and returns only the bootstrap
/// candidates the kernel already computed.
pub trait RelayListPublishSupport: Send + Sync {
    fn build_unsigned_event_from_rows(
        &self,
        rows: &[crate::kernel::AppRelay],
    ) -> Option<nmp_signer_iface::UnsignedEvent>;

    fn cold_start_publish_targets(
        &self,
        declared_rows: &[crate::kernel::AppRelay],
        bootstrap_relays: Vec<String>,
    ) -> Vec<String>;
}

#[derive(Debug, Default)]
pub struct EmptyRelayListPublishSupport;

impl RelayListPublishSupport for EmptyRelayListPublishSupport {
    fn build_unsigned_event_from_rows(
        &self,
        _rows: &[crate::kernel::AppRelay],
    ) -> Option<nmp_signer_iface::UnsignedEvent> {
        None
    }

    fn cold_start_publish_targets(
        &self,
        _declared_rows: &[crate::kernel::AppRelay],
        mut bootstrap_relays: Vec<String>,
    ) -> Vec<String> {
        bootstrap_relays.sort();
        bootstrap_relays.dedup();
        bootstrap_relays
    }
}

#[must_use]
pub fn empty_relay_list_publish_support() -> Arc<dyn RelayListPublishSupport> {
    Arc::new(EmptyRelayListPublishSupport)
}

pub type RelayListPublishSupportSlot = Arc<Mutex<Arc<dyn RelayListPublishSupport>>>;

#[must_use]
pub fn new_relay_list_publish_support_slot() -> RelayListPublishSupportSlot {
    Arc::new(Mutex::new(empty_relay_list_publish_support()))
}
