//! Debt C — capability adapters for `ProtocolCommandContext`.
//!
//! The `Protocol(cmd)` dispatch arm constructs these to bridge the actor's
//! kernel + identity references into the typed capability traits the substrate
//! `ProtocolCommandContext` consumes. Lifetimes are bound to the dispatch arm's
//! stack frame; the adapters never outlive their `RefCell` borrow targets.
//!
//! Extracted from `dispatch.rs` so that file stays within its LOC ceiling — the
//! adapters are a self-contained cluster (five `'a`-lifetime substrate-trait
//! wrappers over a `&RefCell<&mut Kernel>` / `&RefCell<&IdentityRuntime>`).

use std::sync::Arc;

use super::IdentityRuntime;
use crate::kernel::Kernel;

pub(super) struct KernelClockAdapter<'a> {
    pub(super) kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

// SAFETY: the dispatch arm constructs and drops the adapter on the actor
// thread; the `&RefCell<&mut Kernel>` reference never crosses a thread
// boundary. The `Send + Sync` claim is needed because the substrate trait
// carries the bound (`dyn KernelClock` lives behind `&dyn` in
// `ProtocolCommandContext`), but the adapter is held only for the dispatch
// arm's stack frame.
unsafe impl<'a> Send for KernelClockAdapter<'a> {}
unsafe impl<'a> Sync for KernelClockAdapter<'a> {}

impl<'a> crate::substrate::KernelClock for KernelClockAdapter<'a> {
    fn now_secs(&self) -> u64 {
        self.kernel.borrow().now_secs()
    }
}

pub(super) struct LocalSignerAccessAdapter<'a> {
    pub(super) identity: &'a std::cell::RefCell<&'a IdentityRuntime>,
}

unsafe impl<'a> Send for LocalSignerAccessAdapter<'a> {}
unsafe impl<'a> Sync for LocalSignerAccessAdapter<'a> {}

impl<'a> crate::substrate::LocalSignerAccess for LocalSignerAccessAdapter<'a> {
    fn active_local_keys(&self) -> Option<nostr::Keys> {
        self.identity.borrow().active_local_keys().cloned()
    }
    fn signer_for_seal(&self) -> Option<Arc<dyn nmp_nip59::SignerForSeal>> {
        self.identity.borrow().active_signer_for_seal()
    }
}

pub(super) struct ErrorSurfaceAdapter<'a> {
    pub(super) kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

unsafe impl<'a> Send for ErrorSurfaceAdapter<'a> {}
unsafe impl<'a> Sync for ErrorSurfaceAdapter<'a> {}

impl<'a> crate::substrate::ErrorSurface for ErrorSurfaceAdapter<'a> {
    fn set_last_error_toast(&self, message: Option<String>) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.set_last_error_toast(message);
        }
    }
    fn record_action_failure(&self, correlation_id: String, reason: String) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.record_action_failure(correlation_id, reason);
        }
    }
}

pub(super) struct ActionStageTrackerAdapter<'a> {
    pub(super) kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

unsafe impl<'a> Send for ActionStageTrackerAdapter<'a> {}
unsafe impl<'a> Sync for ActionStageTrackerAdapter<'a> {}

impl<'a> crate::substrate::ActionStageTracker for ActionStageTrackerAdapter<'a> {
    fn record_requested(&self, correlation_id: &str) {
        if let Ok(mut k) = self.kernel.try_borrow_mut() {
            k.record_action_stage(
                correlation_id,
                crate::kernel::action_stages::ActionStage::Requested,
                None,
            );
        }
    }
}

/// Debt-C-follow-up — bridge the kernel's `outbox_router` slot into the
/// substrate [`crate::substrate::RecipientRelayLookup`] capability. NIP-57
/// LNURL fetcher consumes this to populate the kind:9734 `relays` tag
/// (recipient's NIP-65 write set + cold-start fallback) without naming
/// `OutboxRouter` or the substrate `MailboxCache` directly.
pub(super) struct RecipientRelayLookupAdapter<'a> {
    pub(super) kernel: &'a std::cell::RefCell<&'a mut Kernel>,
}

unsafe impl<'a> Send for RecipientRelayLookupAdapter<'a> {}
unsafe impl<'a> Sync for RecipientRelayLookupAdapter<'a> {}

impl<'a> crate::substrate::RecipientRelayLookup for RecipientRelayLookupAdapter<'a> {
    fn recipient_publish_relays(&self, recipient: &str, kind: u32) -> Vec<String> {
        // Kernel read; no mutation required. `try_borrow` keeps the adapter
        // total in the presence of a re-entrant kernel borrow on the dispatch
        // arm (defensive — production has no such cycle).
        self.kernel
            .try_borrow()
            .ok()
            .map(|k| k.recipient_publish_relays(recipient, kind))
            .unwrap_or_default()
    }
}
