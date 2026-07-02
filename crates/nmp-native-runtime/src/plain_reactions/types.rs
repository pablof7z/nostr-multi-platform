use nmp_core::substrate::ObservedProjectionReconciler;
use nmp_core::ObservedProjectionId;

/// Runtime handle for one plain-note NIP-25 reaction aggregate read.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Nip25ReactionsHandle {
    pub(super) key: String,
    pub(super) target_event_id: String,
    pub(super) handle_id: u64,
}

impl Nip25ReactionsHandle {
    /// The typed projection key carrying the `nmp.nip25.reactions` payload for
    /// this target.
    #[must_use]
    pub fn projection_key(&self) -> &str {
        &self.key
    }

    #[must_use]
    pub fn target_event_id(&self) -> &str {
        &self.target_event_id
    }
}

/// Teardown state for one live plain-note reaction session.
pub(crate) struct ReactionReadSession {
    pub(super) projection_key: String,
    pub(super) base_observer_id: ObservedProjectionId,
    pub(super) delete_reconciler: ObservedProjectionReconciler,
    pub(super) identity_observer_id: crate::IdentityChangeObserverId,
    pub(super) handle_id: u64,
}
