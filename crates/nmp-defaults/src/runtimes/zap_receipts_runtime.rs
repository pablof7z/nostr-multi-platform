use std::sync::{Arc, Mutex};

use nmp_core::actor::ActorCommand;
use nmp_core::actor::InterestsCommand;
use nmp_core::substrate::{HostCapabilities, IdentityChangeRegistrar};
use nmp_nip57::{self_zap_receipts_identity, self_zap_receipts_interest};

/// Wire the NIP-57 self-zap-receipts subscription runtime into `app`.
pub fn register_zap_receipts_runtime(app: &(impl HostCapabilities + IdentityChangeRegistrar)) {
    let controller = Arc::new(ZapReceiptsRuntimeController {
        active_pubkey: app.active_pubkey(),
        tx: app.actor_sender(),
        last_pushed_pubkey: Mutex::new(None),
    });
    let controller_for_identity = Arc::clone(&controller);
    app.register_identity_change_observer(move |_| controller_for_identity.sync());
    controller.sync();
}

/// Event-driven reconciler for the active-account zap-receipts interest.
pub(crate) struct ZapReceiptsRuntimeController {
    /// Pubkey-only identity slot (Finding C): populated for every backend,
    /// including bunker. Identity only, never secret key material.
    pub(crate) active_pubkey: nmp_core::slots::ActiveAccountSlot,
    pub(crate) tx: nmp_core::CommandSender,
    pub(crate) last_pushed_pubkey: Mutex<Option<String>>,
}

impl ZapReceiptsRuntimeController {
    /// Reconcile the active-account zap-receipts interest after account
    /// changes. Produces no snapshot data, only scoped interest commands.
    pub(crate) fn sync(&self) {
        let active = self.active_pubkey();
        let mut last = self
            .last_pushed_pubkey
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        match (active.as_deref(), last.as_deref()) {
            (Some(now), Some(prev)) if now == prev => {}
            (Some(now), None) => {
                self.ensure(now);
                *last = Some(now.to_string());
            }
            (Some(now), Some(_)) => {
                self.withdraw();
                self.ensure(now);
                *last = Some(now.to_string());
            }
            (None, Some(_)) => {
                self.withdraw();
                *last = None;
            }
            (None, None) => {}
        }
    }

    fn ensure(&self, pubkey: &str) {
        let _ = self
            .tx
            .send(ActorCommand::Interests(InterestsCommand::EnsureInterest {
                identity: self_zap_receipts_identity(),
                interest: self_zap_receipts_interest(pubkey),
            }));
    }

    fn withdraw(&self) {
        let _ = self.tx.send(ActorCommand::Interests(
            InterestsCommand::DropInterestOwner(self_zap_receipts_identity()),
        ));
    }

    fn active_pubkey(&self) -> Option<String> {
        self.active_pubkey.lock().ok().and_then(|slot| slot.clone())
    }
}

#[cfg(test)]
#[path = "../runtimes_zap_tests.rs"]
mod zap_tests;
