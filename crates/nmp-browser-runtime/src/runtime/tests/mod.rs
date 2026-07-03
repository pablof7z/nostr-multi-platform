//! Unit tests for the browser runtime pump loop and full builder->start wiring.
//!
//! The low-level tests drive `pump::drain_inbox` directly with a seeded
//! `KernelReducer` so each `CommandApplyOutcome` arm and the bounded-drain
//! budget are asserted in isolation. The high-level tests go through the public
//! `BrowserAppBuilder` to prove `explicit owner composition` wiring and the command
//! inbox round-trip.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};

use nmp_core::actor::{
    ActorCommand, ActorMail, CipherContinuation, LifecycleCommand, PublishCommand, SignCommand,
};
use nmp_core::{CommandSender, KernelReducer};
use nmp_signer_iface::{SignerOp, UnsignedEvent};
use nmp_signers::{LocalKeySigner, Signer};

use super::event::BrowserRuntimeEvent;
use super::pump::{drain_inbox, DrainInboxContext, BROWSER_COMMAND_DRAIN_BUDGET};
use crate::relay::WakeCell;
use crate::signer::{
    CapabilityProviderRegistry, PendingCipherCompletions, PendingSignerCompletions,
    SignerCompletion, SignerCompletionTx,
};

mod composition;
mod contract;
mod dm_send;
mod feed_attribution;
mod feed_composition;
mod feed_custom_policy;
mod feed_reactivity;
mod feed_simple_groups_reactivity;
mod feed_spec;
#[cfg(feature = "groups")]
mod group_discovery;
#[cfg(feature = "groups")]
mod group_events;
mod pump;
#[cfg(feature = "search")]
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

fn install_test_browser_concepts(app: &mut impl nmp_core::substrate::AppHost) {
    nmp_nip50::register(app, nmp_nip50::Config::default())
        .expect("nmp-nip50 registration must not collide");
    nmp_nip02::register(app, nmp_nip02::Config::default())
        .expect("nmp-nip02 registration must not collide");
    nmp_replies::register(app, nmp_replies::Config::default())
        .expect("nmp-replies registration must not collide");
    nmp_nip25::register(app, nmp_nip25::Config::default())
        .expect("nmp-nip25 registration must not collide");
    nmp_nip18::register(app, nmp_nip18::Config::default())
        .expect("nmp-nip18 registration must not collide");
    nmp_nip84::register(app, nmp_nip84::Config::default())
        .expect("nmp-nip84 registration must not collide");
    nmp_nip29::register(app, nmp_nip29::Config::default())
        .expect("nmp-nip29 registration must not collide");
    nmp_wot::register(app, nmp_wot::Config::default())
        .expect("nmp-wot registration must not collide");
    nmp_nip51::register(
        app,
        nmp_nip51::Config {
            search_fallback_relays: nmp_nip50::SearchFallbackRelays::default(),
        },
    )
    .expect("nmp-nip51 registration must not collide");
    nmp_nip22::register(app, nmp_nip22::Config::default())
        .expect("nmp-nip22 registration must not collide");
    nmp_nip17::register(app, nmp_nip17::Config::default())
        .expect("nmp-nip17 registration must not collide");
    nmp_nip23::register(app, nmp_nip23::Config::default())
        .expect("nmp-nip23 registration must not collide");
}

fn start_test_browser_builder(
    mut builder: crate::BrowserAppBuilder<crate::ProvidersDecided>,
) -> crate::BrowserRuntimeHandle {
    install_test_browser_concepts(&mut builder);
    builder.start()
}

fn test_command_sender() -> CommandSender {
    let (tx, _rx) = mpsc::channel::<ActorMail>();
    CommandSender::new(tx)
}

fn drain_context<'a>(
    pending: &'a mut HashMap<String, super::PendingSignedPublish>,
    registry: &'a CapabilityProviderRegistry,
    pending_signs: &'a mut PendingSignerCompletions,
    pending_ciphers: &'a mut PendingCipherCompletions,
    completion_tx: &'a SignerCompletionTx,
    wake: &'a WakeCell,
    command_sender: &'a CommandSender,
) -> DrainInboxContext<'a> {
    DrainInboxContext {
        pending,
        registry,
        pending_signer_completions: pending_signs,
        pending_cipher_completions: pending_ciphers,
        completion_tx,
        wake,
        command_sender,
    }
}

fn started_handle() -> crate::BrowserRuntimeHandle {
    let builder = crate::BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(crate::BrowserRunConfig::default());
    start_test_browser_builder(builder)
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
    let mut handle = start_test_browser_builder(builder);
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
