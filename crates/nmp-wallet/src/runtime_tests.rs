//! Tests for [`super::WalletRuntime`]: identity-reactive interest
//! open/close, and `on_kernel_event` routing into backend `on_wallet_event`.

use std::sync::Mutex;

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::ObservedProjection;
use nmp_core::ObservedProjectionId;
use nmp_planner::InterestShape;

use super::*;
use crate::backend::{WalletBackend, WalletBackendSnapshot};
use crate::capability::WalletCapabilities;
use crate::projection::WalletReadiness;

#[derive(Clone)]
struct OpenRecord {
    id: ObservedProjectionId,
    consumer_id: String,
    shape: InterestShape,
    observer: Arc<dyn ObservedProjectionSink>,
}

#[derive(Default)]
struct RecordingRegistrar {
    next_id: Mutex<u64>,
    opened: Mutex<Vec<OpenRecord>>,
    closed: Mutex<Vec<ObservedProjectionId>>,
}

impl RecordingRegistrar {
    fn opened(&self) -> Vec<OpenRecord> {
        self.opened.lock().unwrap().clone()
    }

    fn closed(&self) -> Vec<ObservedProjectionId> {
        self.closed.lock().unwrap().clone()
    }
}

impl ObservedProjectionRegistrar for RecordingRegistrar {
    fn open_observed_projection(&self, decl: ObservedProjection) -> ObservedProjectionId {
        let mut next = self.next_id.lock().unwrap();
        *next += 1;
        let id = ObservedProjectionId(*next);
        self.opened.lock().unwrap().push(OpenRecord {
            id,
            consumer_id: decl.consumer_id,
            shape: decl.replay_shapes.into_iter().next().expect("shape"),
            observer: decl.observer,
        });
        id
    }

    fn close_observed_projection(&self, id: ObservedProjectionId) {
        self.closed.lock().unwrap().push(id);
    }

    fn observed_projection_registrar_handle(
        &self,
    ) -> Arc<dyn ObservedProjectionRegistrar + Send + Sync> {
        panic!("test does not request nested registrar handles")
    }
}

/// A minimal `IdentityChangeRegistrar` fake that stores the registered
/// callbacks so the test can fire them directly (mirrors production: the
/// kernel fires these on an actual active-pubkey change, never on an
/// ordinary snapshot tick).
#[derive(Default, Clone)]
struct RecordingIdentityRegistrar {
    callbacks: Arc<Mutex<Vec<Arc<dyn Fn(Option<String>) + Send + Sync>>>>,
}

impl RecordingIdentityRegistrar {
    fn fire(&self, pubkey: Option<String>) {
        for cb in self.callbacks.lock().unwrap().iter() {
            cb(pubkey.clone());
        }
    }
}

impl IdentityChangeRegistrar for RecordingIdentityRegistrar {
    fn register_identity_change_observer<F>(&self, f: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        self.callbacks.lock().unwrap().push(Arc::new(f));
    }
}

#[derive(Default)]
struct FakeApp {
    observed: Arc<RecordingRegistrar>,
    identity: RecordingIdentityRegistrar,
}

impl ObservedProjectionRegistrar for FakeApp {
    fn open_observed_projection(&self, decl: ObservedProjection) -> ObservedProjectionId {
        self.observed.open_observed_projection(decl)
    }

    fn close_observed_projection(&self, id: ObservedProjectionId) {
        self.observed.close_observed_projection(id);
    }

    fn observed_projection_registrar_handle(
        &self,
    ) -> Arc<dyn ObservedProjectionRegistrar + Send + Sync> {
        Arc::clone(&self.observed) as Arc<dyn ObservedProjectionRegistrar + Send + Sync>
    }
}

impl IdentityChangeRegistrar for FakeApp {
    fn register_identity_change_observer<F>(&self, f: F)
    where
        F: Fn(Option<String>) + Send + Sync + 'static,
    {
        self.identity.register_identity_change_observer(f);
    }
}

fn channel() -> (
    nmp_core::CommandSender,
    std::sync::mpsc::Receiver<nmp_core::ActorMail>,
) {
    let (tx, rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    (nmp_core::CommandSender::new(tx), rx)
}

fn unwrap_mail(mail: nmp_core::ActorMail) -> ActorCommand {
    match mail {
        nmp_core::ActorMail::Command(cmd) => cmd,
        other => panic!("expected ActorMail::Command, got {other:?}"),
    }
}

const PK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn nutzap_event(author: &str, p_tag: &str) -> KernelEvent {
    KernelEvent {
        id: "1".repeat(64),
        author: author.to_string(),
        kind: nmp_nip60::kinds::KIND_NIP61_NUTZAP,
        created_at: 12_345,
        tags: vec![vec!["p".to_string(), p_tag.to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

/// A stub backend whose `on_wallet_event` always emits one `ShowToast`, so
/// tests can prove the runtime forwards it onto the command channel.
struct RecordingBackend;

impl WalletBackend for RecordingBackend {
    fn id(&self) -> WalletBackendId {
        WalletBackendId::new("recording")
    }

    fn capabilities(&self) -> WalletCapabilities {
        WalletCapabilities::none()
    }

    fn snapshot(&self, _scope: WalletProjectionScope) -> WalletBackendSnapshot {
        WalletBackendSnapshot {
            projection: WalletProjection::new(
                Some(self.id()),
                WalletReadiness::Ready,
                self.capabilities(),
            ),
        }
    }

    fn start_intent(
        &self,
        _ctx: WalletBackendContext<'_>,
        _intent: crate::backend::WalletIntent,
        _correlation_id: Option<String>,
    ) -> Vec<ActorCommand> {
        Vec::new()
    }

    fn on_wallet_event(
        &self,
        _ctx: WalletBackendContext<'_>,
        _event: &KernelEvent,
    ) -> Vec<ActorCommand> {
        vec![ActorCommand::ShowToast {
            message: "observed".to_string(),
        }]
    }

    fn on_mint_result(
        &self,
        _ctx: WalletBackendContext<'_>,
        _result: MintResult,
    ) -> Vec<ActorCommand> {
        vec![ActorCommand::ShowToast {
            message: "mint-result".to_string(),
        }]
    }
}

#[test]
fn no_interests_open_before_an_account_is_active() {
    let selector = Arc::new(WalletBackendSelector::new(Vec::new()));
    let active_pubkey: ActiveAccountSlot = Arc::new(Mutex::new(None));
    let (tx, _rx) = channel();
    let app = FakeApp::default();

    let _runtime = WalletRuntime::new(selector, active_pubkey, tx, &app);

    assert!(app.observed.opened().is_empty());
}

#[test]
fn sign_in_opens_both_interests_and_logout_closes_them() {
    let selector = Arc::new(WalletBackendSelector::new(Vec::new()));
    let active_pubkey: ActiveAccountSlot = Arc::new(Mutex::new(None));
    let (tx, _rx) = channel();
    let app = FakeApp::default();

    let _runtime = WalletRuntime::new(selector, Arc::clone(&active_pubkey), tx, &app);
    assert!(app.observed.opened().is_empty());

    *active_pubkey.lock().unwrap() = Some(PK.to_string());
    app.identity.fire(Some(PK.to_string()));

    let opened = app.observed.opened();
    assert_eq!(opened.len(), 2, "nutzap + self-authored interests");
    let nutzap = opened
        .iter()
        .find(|o| o.consumer_id == "nmp.wallet.nutzap_receipts")
        .expect("nutzap interest must be opened");
    assert!(nutzap
        .shape
        .kinds
        .contains(&nmp_nip60::kinds::KIND_NIP61_NUTZAP));
    assert!(
        nutzap.id.0 != 0,
        "opened interests must carry a non-zero id"
    );
    let self_authored = opened
        .iter()
        .find(|o| o.consumer_id == "nmp.wallet.self_authored")
        .expect("self-authored interest must be opened");
    assert!(self_authored
        .shape
        .kinds
        .contains(&nmp_nip60::kinds::KIND_NIP60_WALLET));
    assert!(
        self_authored.id.0 != 0,
        "opened interests must carry a non-zero id"
    );
    assert!(app.observed.closed().is_empty());

    *active_pubkey.lock().unwrap() = None;
    app.identity.fire(None);

    let closed = app.observed.closed();
    assert_eq!(
        closed.len(),
        2,
        "logout must close both previously opened interests"
    );
    assert!(closed.contains(&nutzap.id));
    assert!(closed.contains(&self_authored.id));
}

#[test]
fn on_kernel_event_forwards_backend_commands_onto_the_command_channel() {
    let selector = Arc::new(WalletBackendSelector::new(vec![Arc::new(RecordingBackend)]));
    let active_pubkey: ActiveAccountSlot = Arc::new(Mutex::new(Some(PK.to_string())));
    let (tx, rx) = channel();
    let app = FakeApp::default();

    let _runtime = WalletRuntime::new(Arc::clone(&selector), active_pubkey, tx, &app);

    // Fetch the sink the reconciler registered and drive it directly — this
    // is the exact `Arc<dyn ObservedProjectionSink>` the kernel would deliver
    // matching events to.
    let opened = app.observed.opened();
    assert_eq!(opened.len(), 2);
    let sink = opened[0].observer.clone();

    sink.on_kernel_event(&nutzap_event("someone-else", PK));

    let mail = rx
        .recv()
        .expect("the backend's command must have been forwarded");
    assert!(matches!(
        unwrap_mail(mail),
        ActorCommand::ShowToast { message } if message == "observed"
    ));
}

#[test]
fn deliver_mint_result_forwards_the_backends_command() {
    let selector = Arc::new(WalletBackendSelector::new(vec![Arc::new(RecordingBackend)]));
    let active_pubkey: ActiveAccountSlot = Arc::new(Mutex::new(None));
    let (tx, rx) = channel();
    let app = FakeApp::default();

    let runtime = WalletRuntime::new(selector, active_pubkey, tx, &app);

    runtime.deliver_mint_result(
        &WalletBackendId::new("recording"),
        MintResult {
            operation_id: "op-1".to_string(),
            status: crate::backend::MintResultStatus::Settled,
        },
    );
    let mail = rx.recv().expect("a command must have been sent");
    assert!(matches!(
        unwrap_mail(mail),
        ActorCommand::ShowToast { message } if message == "mint-result"
    ));
}

#[test]
fn nutzap_receipts_interest_shape_matches_the_expected_p_tag() {
    let event = nutzap_event("someone-else", PK);
    assert_eq!(event.kind, nmp_nip60::kinds::KIND_NIP61_NUTZAP);
    let shape = nutzap_receipts_shape(PK);
    assert!(shape.kinds.contains(&event.kind));
}
