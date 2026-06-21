//! Unit tests for the LNURL fetcher (`FetchLnurlInvoiceCommand`) — the
//! V-41 migration of the legacy `nmp-core::actor::commands::zap::tests`.
//!
//! HTTP I/O is not exercised here (it needs a live LN provider; the iOS
//! integration shell drives that end-to-end). The tests below pin three
//! observable contracts:
//!
//! 1. V-07 recipient-relay injection — the kind:9734 `relays` tag is
//!    populated from the substrate
//!    [`RecipientRelayLookup`](nmp_core::substrate::RecipientRelayLookup)
//!    capability (kernel-side adapter routes via `outbox_router`); a
//!    pre-existing non-empty `relays` row is preserved.
//! 2. The kind:9734 signer (`sign_zap_request`) round-trips through
//!    `EventBuilder` and rejects out-of-range kinds.
//! 3. The sync-path fail branches in `FetchLnurlInvoiceCommand::run` (no
//!    local keys, sign error) emit the expected `ShowToast` +
//!    `RecordActionFailure` follow-ups through the context's `send`
//!    closure.

use super::*;
use nmp_core::substrate::{
    ActionStageTracker, EmptyDmInboxRelayLookup, KernelClock, LocalSignerAccess, NoopErrorSurface,
    NoopHostOpHandlerAccess, NoopRecipientRelayLookup, NoopWalletKernelAccess, NoopZapProfileLookup,
    ProtocolCommandContextParts, RecipientRelayLookup, SignedEvent,
};

const RECIPIENT_HEX: &str =
    "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

/// ADR-0050 §D3a — the LNURL worker now sends through a `CommandSender`, so the
/// observation receiver carries `ActorMail`. Unwrap the command for the
/// existing assertions.
fn nmp_core_unwrap_mail(mail: nmp_core::ActorMail) -> ActorCommand {
    match mail {
        nmp_core::ActorMail::Command(cmd) => cmd,
        other => panic!("expected ActorMail::Command, got {other:?}"),
    }
}

fn unsigned_for(tags: Vec<Vec<String>>) -> UnsignedEvent {
    UnsignedEvent {
        pubkey: String::new(),
        kind: 9734,
        tags,
        content: String::new(),
        created_at: 0,
    }
}

// ── Capability adapters used by the LNURL test harness ──

struct FixedClock(u64);
impl KernelClock for FixedClock {
    fn now_secs(&self) -> u64 {
        self.0
    }
}

/// Noop [`LocalSignerAccess`] for the relay-injection tests. V-78 reconcile:
/// `FetchLnurlInvoiceCommand::run` no longer signs through this trait — signing
/// now leaves the command via the `ActorCommand::SignEventForAccount` port,
/// which the actor's dispatch arm resolves (proven in `nmp-core`'s
/// `sign_event_for_account_tests`). So the test signer only needs to satisfy
/// the (now sign-free) trait surface; the relay-injection tests never sign at
/// all. The sign-path tests instead pull the port command's continuation out of
/// the captured `send` and drive it directly (see [`run_with_signer`]).
struct LocalSigner;

impl LocalSigner {
    /// Back-compat constructor for the existing relay-injection tests that
    /// wrote `LocalSigner::none()` (they never sign).
    fn none() -> Self {
        Self
    }
}

impl LocalSignerAccess for LocalSigner {
    fn active_local_keys(&self) -> Option<Keys> {
        None
    }
    fn active_account_pubkey(&self) -> Option<String> {
        None
    }
}

/// Mint the `SignedEvent` the dispatch arm's port would hand to the
/// continuation for a local-nsec / bunker account signing `unsigned` with
/// `keys`. Used by the sign-path tests to drive the captured continuation
/// (modelling what the actor's `SignEventForAccount` dispatch arm produces —
/// proven backend-transparent in `nmp-core`'s `sign_event_for_account_tests`).
fn signed_for(keys: &Keys, unsigned: &UnsignedEvent) -> SignedEvent {
    let json = sign_zap_request(keys, unsigned).expect("sign must succeed");
    let event: nostr::Event = serde_json::from_str(&json).expect("valid event");
    nostr_event_to_signed(&event)
}

/// Flatten a signed `nostr::Event` into the substrate [`SignedEvent`] the
/// signer seam produces — the inverse of `signed_event_to_nostr_json`.
fn nostr_event_to_signed(event: &nostr::Event) -> SignedEvent {
    SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: u32::from(event.kind.as_u16()),
            tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    }
}

struct RecordingStages(std::sync::Mutex<Vec<String>>);
impl ActionStageTracker for RecordingStages {
    fn record_requested(&self, correlation_id: &str) {
        self.0.lock().unwrap().push(correlation_id.to_string());
    }
}

/// Test-only [`RecipientRelayLookup`] returning a fixed URL list for
/// every recipient. Records every `(recipient, kind)` it was asked
/// about so tests can assert on the routing call shape.
struct FixedRecipientLookup {
    urls: Vec<String>,
    seen: std::sync::Mutex<Vec<(String, u32)>>,
}

impl FixedRecipientLookup {
    fn with_urls(urls: Vec<&'static str>) -> Self {
        Self {
            urls: urls.into_iter().map(str::to_string).collect(),
            seen: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl RecipientRelayLookup for FixedRecipientLookup {
    fn recipient_publish_relays(&self, recipient: &str, kind: u32) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .push((recipient.to_string(), kind));
        self.urls.clone()
    }
}

/// Build a `ProtocolCommandContext` whose kernel accessors are wired to fixed
/// capability adapters. `command_sender` backs
/// [`ProtocolCommandContext::command_sender_clone`] — the sign-path tests pass a
/// sender whose receiver they keep so the continuation's worker `send`s are
/// observable; the relay-injection tests pass a throwaway (the [`ctx_with`]
/// default). The DM-inbox / toast / failure / wallet / zap surfaces use Noops.
fn ctx_with_sender<'a>(
    send: &'a dyn Fn(ActorCommand),
    command_sender: nmp_core::CommandSender,
    clock: &'a dyn KernelClock,
    signers: &'a LocalSigner,
    stages: &'a RecordingStages,
    recipients: &'a dyn RecipientRelayLookup,
) -> ProtocolCommandContext<'a> {
    static EMPTY_DM: EmptyDmInboxRelayLookup = EmptyDmInboxRelayLookup;
    static ERRORS: NoopErrorSurface = NoopErrorSurface;
    static HOST_OP: NoopHostOpHandlerAccess = NoopHostOpHandlerAccess;
    static WALLET: NoopWalletKernelAccess = NoopWalletKernelAccess;
    static ZAP: NoopZapProfileLookup = NoopZapProfileLookup;
    ProtocolCommandContext::new(ProtocolCommandContextParts {
        send,
        command_sender,
        clock,
        signers,
        dms: &EMPTY_DM,
        errors: &ERRORS,
        stages,
        recipients,
        host_op_handler: &HOST_OP,
        wallet_kernel: &WALLET,
        zap_profiles: &ZAP,
    })
}

/// [`ctx_with_sender`] with a throwaway command sender (receiver dropped, so
/// worker `send`s are benign no-ops). Used by the relay-injection tests, which
/// never spawn a worker.
fn ctx_with<'a>(
    send: &'a dyn Fn(ActorCommand),
    clock: &'a dyn KernelClock,
    signers: &'a LocalSigner,
    stages: &'a RecordingStages,
    recipients: &'a dyn RecipientRelayLookup,
) -> ProtocolCommandContext<'a> {
    let (tx, _rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    ctx_with_sender(send, nmp_core::CommandSender::new(tx), clock, signers, stages, recipients)
}

// ────────────────────────────────────────────────────────────────────
// V-07 — recipient relay injection through the protocol context.
// ────────────────────────────────────────────────────────────────────

#[test]
fn inject_recipient_relays_preserves_existing_relays_tag() {
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner::none();
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients =
        FixedRecipientLookup::with_urls(vec!["wss://from-router.example"]);
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned = unsigned_for(vec![
        vec!["relays".to_string(), "wss://chosen.example".to_string()],
        vec!["p".to_string(), RECIPIENT_HEX.to_string()],
    ]);
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_tag = unsigned
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("relays"))
        .expect("relays tag must be present");
    assert_eq!(
        relays_tag,
        &vec!["relays".to_string(), "wss://chosen.example".to_string()],
        "an explicit non-empty relays tag must be left untouched"
    );
    let relays_count = unsigned
        .tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("relays"))
        .count();
    assert_eq!(relays_count, 1, "must not duplicate the relays tag");
    // And the router must NOT have been consulted — the caller's tag wins.
    assert!(
        recipients.seen.lock().unwrap().is_empty(),
        "router must not be consulted when a filled relays row is present"
    );
}

#[test]
fn inject_recipient_relays_injects_when_tag_absent() {
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner::none();
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients = FixedRecipientLookup::with_urls(vec![
        "wss://write-a.example",
        "wss://write-b.example",
    ]);
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned =
        unsigned_for(vec![vec!["p".to_string(), RECIPIENT_HEX.to_string()]]);
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_tag = unsigned
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("relays"))
        .expect("V-07: actor must inject a relays tag when caller omitted it");
    assert_eq!(
        relays_tag,
        &vec![
            "relays".to_string(),
            "wss://write-a.example".to_string(),
            "wss://write-b.example".to_string(),
        ],
        "must inject every router-resolved URL into the relays row"
    );
    // The router was asked once, for kind:9735 (the zap receipt the LN
    // provider will mint — that's the kind whose publish-direction routes
    // to the recipient's NIP-65 write set).
    assert_eq!(
        *recipients.seen.lock().unwrap(),
        vec![(RECIPIENT_HEX.to_string(), 9735u32)],
        "router must be asked for kind:9735 against the p-tag recipient"
    );
}

#[test]
fn inject_recipient_relays_treats_bare_relays_key_as_absent() {
    // A `["relays"]` row with no URLs is malformed — treat as absent so
    // the injection still fires, AND the malformed row must be discarded.
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner::none();
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients =
        FixedRecipientLookup::with_urls(vec!["wss://write.example"]);
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned = unsigned_for(vec![
        vec!["relays".to_string()],
        vec!["p".to_string(), RECIPIENT_HEX.to_string()],
    ]);
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_rows: Vec<&Vec<String>> = unsigned
        .tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("relays"))
        .collect();
    assert_eq!(
        relays_rows.len(),
        1,
        "must end up with exactly one relays row (the bare one is dropped)"
    );
    assert!(
        relays_rows[0].len() > 1,
        "the surviving relays row must carry the injected URLs: {:?}",
        relays_rows[0]
    );
}

#[test]
fn inject_recipient_relays_falls_back_to_bootstrap_when_p_tag_missing() {
    // Defensive — a builder bug that drops the `p` tag must NOT produce
    // a zap with an empty relays tag. The router resolves the empty
    // recipient against its cold-start AppRelay seed (lane 7) — the test
    // wires that resolution through the `FixedRecipientLookup` adapter
    // (which models the router's lane-7 fallback).
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner::none();
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients = FixedRecipientLookup::with_urls(vec![
        "wss://bootstrap.example",
    ]);
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned = unsigned_for(Vec::new());
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_tag = unsigned
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("relays"))
        .expect("must inject a relays tag even when p tag is absent");
    assert_eq!(
        relays_tag,
        &vec![
            "relays".to_string(),
            "wss://bootstrap.example".to_string(),
        ],
        "router-resolved URLs (router's own cold-start lane) populate the tag"
    );
    // The router was consulted with an empty recipient pubkey — the LNURL
    // fetcher does not synthesise a fake recipient when the `p` tag is
    // missing; routing decides the fallback (lane 7 in production).
    assert_eq!(
        *recipients.seen.lock().unwrap(),
        vec![(String::new(), 9735u32)],
        "router asked with empty recipient when p tag missing"
    );
}

#[test]
fn inject_recipient_relays_emits_empty_tag_when_router_returns_no_urls() {
    // Documented behaviour from the function doc comment: if the router
    // returns an empty `Vec` (e.g. `RoutingError::Unroutable` — no NIP-65
    // cache hit AND no AppRelay seed), the `relays` tag is still added,
    // empty. The LN provider then falls back to its own default; the
    // contract NIP-57 § "Appendix A" wants the tag PRESENT.
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner::none();
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients = NoopRecipientRelayLookup;
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned =
        unsigned_for(vec![vec!["p".to_string(), RECIPIENT_HEX.to_string()]]);
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_tag = unsigned
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("relays"))
        .expect("relays row must be added even with an empty URL set");
    assert_eq!(
        relays_tag,
        &vec!["relays".to_string()],
        "empty router result yields a bare relays row (LN provider falls back)"
    );
}

// ────────────────────────────────────────────────────────────────────
// `sign_zap_request` — round-trip + kind range.
// ────────────────────────────────────────────────────────────────────

#[test]
fn sign_zap_request_round_trips_through_event_builder() {
    let keys = Keys::generate();
    let unsigned = UnsignedEvent {
        pubkey: keys.public_key().to_hex(),
        kind: 9734,
        tags: vec![
            vec![
                "relays".to_string(),
                "wss://relay.example".to_string(),
            ],
            vec![
                "p".to_string(),
                "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff".to_string(),
            ],
        ],
        content: "great post 🤙".to_string(),
        created_at: 1_700_000_000,
    };
    let json = sign_zap_request(&keys, &unsigned).expect("sign must succeed");
    let event: nostr::Event =
        serde_json::from_str(&json).expect("signed output must be a valid nostr::Event");
    assert_eq!(event.kind.as_u16(), 9734);
    assert_eq!(event.content, "great post 🤙");
    assert!(!event.sig.to_string().is_empty());
}

#[test]
fn sign_zap_request_rejects_out_of_range_kind() {
    let keys = Keys::generate();
    let unsigned = UnsignedEvent {
        pubkey: keys.public_key().to_hex(),
        // 100_000 is outside the u16 range nostr::Kind accepts.
        kind: 100_000,
        tags: Vec::new(),
        content: String::new(),
        created_at: 0,
    };
    assert!(sign_zap_request(&keys, &unsigned).is_err());
}

// ────────────────────────────────────────────────────────────────────
// `FetchLnurlInvoiceCommand::run` — sign-path branches (V-78 reconcile).
//
// `run` emits `SignEventForAccount` (the port); its continuation spawns the
// HTTP worker. Driving with `Ok(signed)` spawns the worker; `Err` fails
// closed. Backend-transparent resolution is proven in nmp-core.
// ────────────────────────────────────────────────────────────────────

use std::sync::Mutex;

/// Drive `run()` with a captured send sink + recordable stage tracker.
struct Sink {
    sends: Mutex<Vec<ActorCommand>>,
    stages: Mutex<Vec<String>>,
}

impl Sink {
    fn new() -> Self {
        Self {
            sends: Mutex::new(Vec::new()),
            stages: Mutex::new(Vec::new()),
        }
    }
}

/// Valid lightning address whose HTTP leg will fail (domain nonexistent) so the
/// sign-path tests stay hermetic while passing the `inject_lnurl_tag` gate.
const UNREACHABLE_LNURL: &str = "zap-test@unreachable.invalid";

struct PortCapture {
    signer_pubkey: Option<String>,
    unsigned: UnsignedEvent,
    continuation: nmp_core::SignContinuation,
    stages: Vec<String>,
    worker_rx: std::sync::mpsc::Receiver<nmp_core::ActorMail>,
}

fn run_and_capture_port(correlation_id: Option<String>) -> PortCapture {
    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = FixedClock(1_700_000_000);
    let stages = RecordingStages(Mutex::new(Vec::new()));
    let recipients = NoopRecipientRelayLookup;
    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<nmp_core::ActorMail>();
    let signers = LocalSigner::none();
    {
        let mut ctx = ctx_with_sender(&send, nmp_core::CommandSender::new(worker_tx), &clock, &signers, &stages, &recipients);
        let cmd = Box::new(FetchLnurlInvoiceCommand {
            unsigned: unsigned_for(vec![vec!["p".to_string(), RECIPIENT_HEX.to_string()]]),
            recipient_pubkey: RECIPIENT_HEX.to_string(),
            lnurl_or_address: Some(UNREACHABLE_LNURL.to_string()),
            amount_msats: 21_000,
            correlation_id,
            // ADR-0052 rung 5.2: this test exercises the sign/LNURL legs, not
            // the wallet handoff — no payment port wired.
            payment_port: None,
        });
        cmd.run(&mut ctx).expect("run returns Ok");
    }
    let mut sends = sink.sends.into_inner().unwrap();
    assert_eq!(sends.len(), 1, "run must emit exactly one SignEventForAccount: {sends:?}");
    let (signer_pubkey, unsigned, continuation) = match sends.remove(0) {
        ActorCommand::SignEventForAccount { signer_pubkey, unsigned, continuation } => (signer_pubkey, unsigned, continuation),
        other => panic!("expected SignEventForAccount, got {other:?}"),
    };
    PortCapture { signer_pubkey, unsigned, continuation, stages: stages.0.into_inner().unwrap(), worker_rx }
}

/// `run` must sign the kind:9734 through the unified `SignEventForAccount`
/// port with the ACTIVE account (`signer_pubkey: None`), AFTER recording the
/// Requested stage. The unsigned event carried into the port is the kind:9734
/// with `created_at` re-stamped from the context clock (D7).
#[test]
fn run_emits_sign_event_for_account_port_with_active_account() {
    let cap = run_and_capture_port(Some("cid-port".to_string()));
    assert_eq!(
        cap.signer_pubkey, None,
        "the zap request signs with the ACTIVE account (signer_pubkey = None)"
    );
    assert_eq!(cap.unsigned.kind, 9734, "the port carries the kind:9734 zap request");
    assert_eq!(
        cap.unsigned.created_at, 1_700_000_000,
        "created_at must be re-stamped from the context clock (D7)"
    );
    assert_eq!(
        cap.stages,
        vec!["cid-port".to_string()],
        "Requested stage must record once before the sign port command"
    );
}

/// V-78 reconcile — the genuine no-account / sign-error case still fails
/// closed: driving the port continuation with `Err(reason)` emits the toast +
/// `RecordActionFailure` through the worker channel. (In production the
/// dispatch arm produces this `Err` when there is no active account.)
#[test]
fn continuation_err_fails_closed_with_toast_and_failure() {
    let cap = run_and_capture_port(Some("cid-none".to_string()));
    cap.continuation
        .call(Err("no active account — sign in first".to_string()));

    let sends: Vec<ActorCommand> = cap.worker_rx.try_iter().map(nmp_core_unwrap_mail).collect();
    assert_eq!(
        sends.len(),
        2,
        "expected ShowToast + RecordActionFailure: {sends:?}"
    );
    match &sends[0] {
        ActorCommand::ShowToast { message } => {
            assert!(
                message.to_lowercase().contains("zap failed"),
                "toast must surface the zap failure: {message}"
            );
        }
        other => panic!("expected ShowToast, got {other:?}"),
    }
    match &sends[1] {
        ActorCommand::RecordActionFailure { correlation_id, .. } => {
            assert_eq!(correlation_id, "cid-none");
        }
        other => panic!("expected RecordActionFailure, got {other:?}"),
    }
}

/// Without a `correlation_id`, a sign-error continuation emits ONLY the toast
/// (no `RecordActionFailure` — there is no spinner to clear).
#[test]
fn continuation_err_without_correlation_emits_only_toast() {
    let cap = run_and_capture_port(None);
    assert!(cap.stages.is_empty(), "no correlation_id → no Requested stage");
    cap.continuation.call(Err("no active account".to_string()));

    let sends: Vec<ActorCommand> = cap.worker_rx.try_iter().map(nmp_core_unwrap_mail).collect();
    assert_eq!(sends.len(), 1, "expected only ShowToast: {sends:?}");
    assert!(matches!(&sends[0], ActorCommand::ShowToast { .. }));
}

/// V-78 reconcile — the core proof. Whether the active account is a local
/// nsec or a NIP-46 bunker, the dispatch arm resolves the port to the SAME
/// `Ok(signed)` (proven backend-transparent in `nmp-core`). Driving this
/// command's continuation with that `Ok(signed)` spawns the off-actor HTTP
/// worker — NO synchronous fail-closed toast/failure. The worker fails at the
/// parse-failing LNURL leg (off-thread), surfacing a `ShowToast` +
/// `RecordActionFailure` through the worker channel, NOT a "no local keys"
/// rejection. This is the bunker zap working through the unified seam.
#[test]
fn continuation_ok_spawns_worker_carrying_signed_event() {
    let cap = run_and_capture_port(Some("cid-ok".to_string()));
    // Mint the SignedEvent the dispatch arm's port hands back for this
    // kind:9734 — identical shape for local nsec and bunker (the backend is
    // invisible past the port).
    let keys = Keys::generate();
    let signed = signed_for(&keys, &cap.unsigned);
    cap.continuation.call(Ok(signed));

    // The continuation spawned the HTTP worker; it fails at the unreachable
    // LNURL parse and posts its terminal through the worker channel. Block
    // briefly on the worker's first message so the off-thread send is observed
    // deterministically (no polling).
    let first = nmp_core_unwrap_mail(
        cap.worker_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("worker must post a terminal after the LNURL leg fails"),
    );
    match first {
        ActorCommand::ShowToast { message } => {
            assert!(
                message.to_lowercase().contains("zap failed"),
                "worker surfaces the LNURL failure (NOT a no-local-keys rejection): {message}"
            );
        }
        other => panic!("expected ShowToast from the worker, got {other:?}"),
    }
    // The matching RecordActionFailure follows (correlation present).
    let second = nmp_core_unwrap_mail(
        cap.worker_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("RecordActionFailure must follow when correlation_id is present"),
    );
    match second {
        ActorCommand::RecordActionFailure { correlation_id, .. } => {
            assert_eq!(correlation_id, "cid-ok");
        }
        other => panic!("expected RecordActionFailure, got {other:?}"),
    }
}

/// V-78 — `signed_event_to_nostr_json` must reproduce the EXACT flat NIP-01
/// wire bytes `sign_zap_request` emits, so a bunker-signed kind:9734 hits the
/// LN provider's callback byte-for-byte identical to a local-nsec zap. The
/// signed `nostr::Event` is flattened to `SignedEvent` and rebuilt; the two
/// serializations must be equal.
#[test]
fn signed_event_to_nostr_json_matches_sign_zap_request_bytes() {
    let keys = Keys::generate();
    let unsigned = UnsignedEvent {
        pubkey: keys.public_key().to_hex(),
        kind: 9734,
        tags: vec![
            vec!["relays".to_string(), "wss://relay.example".to_string()],
            vec!["p".to_string(), RECIPIENT_HEX.to_string()],
            vec!["amount".to_string(), "21000".to_string()],
        ],
        content: "nice post 🤙".to_string(),
        created_at: 1_700_000_000,
    };

    // The canonical local path.
    let direct = sign_zap_request(&keys, &unsigned).expect("sign must succeed");
    // Flatten that signed event into a substrate SignedEvent, then rebuild
    // the flat JSON through the V-78 helper.
    let event: nostr::Event = serde_json::from_str(&direct).expect("valid event");
    let signed = nostr_event_to_signed(&event);
    let rebuilt = signed_event_to_nostr_json(&signed).expect("rebuild must succeed");

    assert_eq!(
        direct, rebuilt,
        "the bunker-rebuilt flat NIP-01 JSON must be byte-identical to the \
         local-nsec sign output"
    );
}

#[test]
fn run_restamps_created_at_from_context_clock() {
    // Indirect: we can't observe `unsigned.created_at` after the move,
    // but we can verify the dispatch arm calls `now_secs` once when the
    // sentinel is `0`. Wire a counter through a custom clock adapter.
    use std::sync::atomic::{AtomicU64, Ordering};

    struct CountingClock(AtomicU64);
    impl KernelClock for CountingClock {
        fn now_secs(&self) -> u64 {
            self.0.fetch_add(1, Ordering::SeqCst);
            1_700_000_000
        }
    }

    let sink = Sink::new();
    let send = |c: ActorCommand| sink.sends.lock().unwrap().push(c);
    let clock = CountingClock(AtomicU64::new(0));
    let signers = LocalSigner::none();
    let stages = RecordingStages(Mutex::new(Vec::new()));
    let recipients = NoopRecipientRelayLookup;
    let mut ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let cmd = Box::new(FetchLnurlInvoiceCommand {
        unsigned: unsigned_for(vec![
            vec!["p".to_string(), RECIPIENT_HEX.to_string()],
        ]),
        recipient_pubkey: RECIPIENT_HEX.to_string(),
        lnurl_or_address: Some("alice@example.com".to_string()),
        amount_msats: 21_000,
        correlation_id: None,
        // ADR-0052 rung 5.2: sign/LNURL-leg test, no payment port wired.
        payment_port: None,
    });
    cmd.run(&mut ctx).expect("run returns Ok on fail-closed branch");
    assert!(
        clock.0.load(Ordering::SeqCst) >= 1,
        "now_secs must be invoked when created_at sentinel is 0"
    );
}

// ────────────────────────────────────────────────────────────────────
// NIP-57 bolt11 amount validation — fail-closed guard before wallet pay.
//
// A malicious or buggy LNURL provider can return a bolt11 whose encoded
// amount differs from the user-requested amount.  `validate_bolt11_amount`
// MUST reject any mismatch and MUST reject amountless invoices (fail closed —
// an unverifiable invoice must never be auto-paid).  Only an exact match on
// a parseable amount proceeds.
// ────────────────────────────────────────────────────────────────────

/// Build a minimal fake bolt11 invoice string whose HRP encodes `msats`
/// millisatoshis.  The data part ("1pvjluez000") is syntactically sufficient
/// for the BOLT-11 HRP parser (`crate::bolt11::amount_msats`) — we do not need
/// a cryptographically valid invoice for these unit tests.
fn fake_bolt11_for_msats(msats: u64) -> String {
    // Convert msats to the most compact BOLT-11 HRP representation.
    // Use the `n` (nano-BTC) multiplier: 1 nBTC = 100 msat, so any multiple
    // of 100 is exactly representable.  All zap amounts used in the tests are
    // multiples of 100 msat.
    const MSATS_PER_NANOBTC: u64 = 100;
    let n = msats / MSATS_PER_NANOBTC;
    format!("lnbc{n}n1pvjluez000")
}

/// A bolt11 with NO amount in the HRP (amountless, per BOLT-11 optional-amount
/// spec).  The `crate::bolt11::amount_msats` parser returns `None` for this shape.
const AMOUNTLESS_BOLT11: &str = "lnbc1pvjluez000";

#[test]
fn validate_bolt11_amount_accepts_exact_match() {
    // 21_000 msat = 210 nBTC.
    let bolt11 = fake_bolt11_for_msats(21_000);
    assert_eq!(
        validate_bolt11_amount(&bolt11, 21_000),
        Ok(()),
        "an invoice whose decoded amount exactly equals the requested amount must succeed"
    );
}

#[test]
fn validate_bolt11_amount_rejects_higher_amount() {
    // Provider encodes 42_000 msat but user requested 21_000 — would silently
    // double-charge the user.
    let bolt11 = fake_bolt11_for_msats(42_000);
    let result = validate_bolt11_amount(&bolt11, 21_000);
    assert!(
        result.is_err(),
        "invoice encoding MORE than requested must be rejected: {result:?}"
    );
    let msg = result.unwrap_err();
    assert!(
        msg.contains("21000") && msg.contains("42000"),
        "error must name both the requested and actual amounts: {msg}"
    );
}

#[test]
fn validate_bolt11_amount_rejects_lower_amount() {
    // Provider encodes 1_000 msat but user requested 21_000 — still wrong.
    let bolt11 = fake_bolt11_for_msats(1_000);
    let result = validate_bolt11_amount(&bolt11, 21_000);
    assert!(
        result.is_err(),
        "invoice encoding LESS than requested must be rejected: {result:?}"
    );
    let msg = result.unwrap_err();
    assert!(
        msg.contains("21000") && msg.contains("1000"),
        "error must name both amounts: {msg}"
    );
}

#[test]
fn validate_bolt11_amount_rejects_amountless_invoice() {
    // Fail closed — an invoice with no parseable amount must NEVER be auto-paid.
    // The user chose an explicit amount; an amountless invoice gives no proof
    // the provider will charge only that amount.
    let result = validate_bolt11_amount(AMOUNTLESS_BOLT11, 21_000);
    assert!(
        result.is_err(),
        "an amountless invoice must be rejected (fail closed): {result:?}"
    );
}

#[test]
fn validate_bolt11_amount_rejects_malformed_amount_hrp() {
    // An invoice that passes `looks_like_bolt11` (correct prefix) but has a
    // malformed amount HRP (non-digit chars) still fails validation.
    let result = validate_bolt11_amount("lnbc5x0u1pvjluez000", 21_000);
    assert!(
        result.is_err(),
        "an invoice with a malformed amount must be rejected: {result:?}"
    );
}

// ────────────────────────────────────────────────────────────────────
// `inject_lnurl_tag` — fail-closed contract (D6 fix).
// Before the fix both error paths silently `return`'d; the fix returns
// `Err` so the caller can abort the zap rather than proceed without the tag.
// ────────────────────────────────────────────────────────────────────

/// Valid lightning address → injects a bech32 lnurl1… tag.
#[test]
fn inject_lnurl_tag_inserts_tag_for_valid_lightning_address() {
    let mut u = unsigned_for(vec![vec!["p".to_string(), RECIPIENT_HEX.to_string()]]);
    assert!(inject_lnurl_tag("alice@pay.example.com", &mut u).is_ok());
    let row = u.tags.iter().find(|t| t.first().map(String::as_str) == Some("lnurl"))
        .expect("lnurl tag must be injected");
    assert!(row.len() > 1 && row[1].starts_with("lnurl1"), "tag must be bech32: {row:?}");
}

/// Unparseable input → Err (caller aborts the zap), no tag added.
#[test]
fn inject_lnurl_tag_returns_err_for_unparseable_input() {
    let mut u = unsigned_for(vec![vec!["p".to_string(), RECIPIENT_HEX.to_string()]]);
    assert!(
        inject_lnurl_tag("not-a-valid-lnurl-at-all", &mut u).is_err(),
        "unparseable input must return Err"
    );
    assert!(!u.tags.iter().any(|t| t.first().map(String::as_str) == Some("lnurl")));
}

/// Existing non-empty lnurl tag → no-op (Ok, tag unchanged, no duplicate).
#[test]
fn inject_lnurl_tag_skips_when_tag_already_present() {
    let existing = vec!["lnurl".to_string(), "lnurl1dp68gurn8ghj7arg9ekxzar9wd6xzarfwfjhgwf3h".to_string()];
    let mut u = unsigned_for(vec![existing.clone(), vec!["p".to_string(), RECIPIENT_HEX.to_string()]]);
    assert!(inject_lnurl_tag("alice@pay.example.com", &mut u).is_ok());
    let rows: Vec<_> = u.tags.iter().filter(|t| t.first().map(String::as_str) == Some("lnurl")).collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(*rows[0], existing);
}
