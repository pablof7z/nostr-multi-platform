//! Unit tests for the browser runtime pump loop and full builder->start wiring.
//!
//! The low-level tests drive `pump::drain_inbox` directly with a seeded
//! `KernelReducer` so each `CommandApplyOutcome` arm and the bounded-drain
//! budget are asserted in isolation. The high-level tests go through the public
//! `BrowserAppBuilder` to prove `register_defaults` wiring and the command
//! inbox round-trip.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{mpsc, Arc};

use nmp_core::actor::{ActorCommand, ActorMail, LifecycleCommand, PublishCommand};
use nmp_core::KernelReducer;
use nmp_signer_iface::{SignerOp, UnsignedEvent};
use nmp_signers::{LocalKeySigner, Signer};

use super::event::BrowserRuntimeEvent;
use super::pump::{drain_inbox, BROWSER_COMMAND_DRAIN_BUDGET};
use crate::relay::WakeCell;
use crate::signer::{CapabilityProviderRegistry, SignerCompletion, SignerCompletionTx};

mod composition;
mod contract;
mod feed_attribution;
mod feed_composition;
mod group_timeline;
mod pump;
mod search;
mod signer;

fn enqueue(cmds: Vec<ActorCommand>) -> mpsc::Receiver<ActorMail> {
    let (tx, rx) = mpsc::channel::<ActorMail>();
    for c in cmds {
        tx.send(ActorMail::Command(c)).expect("send");
    }
    // Drop `tx`: a disconnected-but-non-empty channel still drains every queued
    // item before `Disconnected` is observed, matching the live runtime where
    // the sender outlives the drain.
    rx
}

fn empty_broker() -> (CapabilityProviderRegistry, SignerCompletionTx) {
    let reg = CapabilityProviderRegistry::new();
    let (tx, _rx) = mpsc::channel::<SignerCompletion>();
    (reg, tx)
}

fn noop_wake() -> WakeCell {
    Rc::new(RefCell::new(Rc::new(|| {}) as Rc<dyn Fn()>))
}

fn started_handle() -> crate::BrowserRuntimeHandle {
    crate::BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(crate::BrowserRunConfig::default())
        .start()
}

fn handle_with_local_key_signer() -> (crate::BrowserRuntimeHandle, String) {
    let signer = LocalKeySigner::from_secret_hex(&"ee".repeat(32)).expect("valid secret");
    let pubkey_hex = signer.pubkey().to_hex();
    let signer: Arc<dyn Signer> = Arc::new(signer);

    let builder = crate::BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(crate::BrowserRunConfig::default());
    builder.with_capability_providers([Arc::clone(&signer)]);
    let mut handle = builder.start();
    handle.set_active_account_for_test(pubkey_hex.clone());
    (handle, pubkey_hex)
}

fn install_counting_wake(handle: &mut crate::BrowserRuntimeHandle) -> Rc<Cell<u32>> {
    let count = Rc::new(Cell::new(0u32));
    let count_clone = Rc::clone(&count);
    handle.set_wake(Rc::new(move || {
        count_clone.set(count_clone.get() + 1);
    }));
    count
}

fn park_host_brokered_sign(
    handle: &mut crate::BrowserRuntimeHandle,
    correlation_id: &str,
) -> (String, String) {
    let sender = handle.command_sender();
    sender
        .send(ActorCommand::Publish(PublishCommand::Profile {
            fields: serde_json::Map::new(),
            correlation_id: Some(correlation_id.to_string()),
        }))
        .expect("send");
    let out = handle.pump();
    let sign_req = out
        .events
        .iter()
        .find(|e| matches!(e, BrowserRuntimeEvent::SignRequest { .. }))
        .expect("no-provider path must emit SignRequest");
    let BrowserRuntimeEvent::SignRequest {
        correlation_id,
        unsigned_json,
        ..
    } = sign_req
    else {
        unreachable!()
    };
    (correlation_id.clone(), unsigned_json.clone())
}
