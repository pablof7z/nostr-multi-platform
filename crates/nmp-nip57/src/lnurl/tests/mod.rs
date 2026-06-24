//! Unit tests for the LNURL fetcher (`FetchLnurlInvoiceCommand`) — the
//! V-41 migration of the legacy `nmp-core::actor::commands::zap::tests`.
//!
//! HTTP I/O is not exercised here (it needs a live LN provider; the iOS
//! integration shell drives that end-to-end). Split into cohesive submodules
//! (AGENTS.md 500-LOC ceiling); each pins one observable contract:
//!
//! - [`relay_injection`] — V-07 recipient-relay injection: the kind:9734
//!   `relays` tag is populated from the substrate
//!   [`RecipientRelayLookup`](nmp_core::substrate::RecipientRelayLookup)
//!   capability; a pre-existing non-empty `relays` row is preserved.
//! - [`sign_zap_request`] — the kind:9734 signer round-trips through
//!   `EventBuilder`, rejects out-of-range kinds, and produces byte-identical
//!   flat NIP-01 wire bytes whether signed locally or via the V-78 port.
//! - [`port_run`] — `FetchLnurlInvoiceCommand::run` emits the
//!   `SignEventForAccount` port and its continuation spawns the worker / fails
//!   closed.
//! - [`bolt11_validation`] — `validate_bolt11_amount` + `inject_lnurl_tag`
//!   fail-closed guards.
//!
//! This module owns the shared test harness (capability adapters + context
//! builders); the submodules reach it (and the `lnurl` internals under test)
//! through `use super::*`.

use super::*;
use nmp_core::substrate::{
    ActionStageTracker, EmptyDmInboxRelayLookup, KernelClock, LocalSignerAccess, NoopErrorSurface,
    NoopRecipientRelayLookup, NoopWalletKernelAccess, NoopZapProfileLookup,
    ProtocolCommandContextParts, RecipientRelayLookup,
};
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

mod bolt11_validation;
mod port_run;
mod relay_injection;
mod sign_zap_request;

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
/// the captured `send` and drive it directly.
struct LocalSigner;

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
    static WALLET: NoopWalletKernelAccess = NoopWalletKernelAccess;
    static ZAP: NoopZapProfileLookup = NoopZapProfileLookup;
    static WRITE_RELAYS: nmp_core::substrate::NoopWriteRelayLookup =
        nmp_core::substrate::NoopWriteRelayLookup;
    ProtocolCommandContext::new(ProtocolCommandContextParts {
        send,
        command_sender,
        clock,
        signers,
        dms: &EMPTY_DM,
        errors: &ERRORS,
        stages,
        recipients,
        wallet_kernel: &WALLET,
        zap_profiles: &ZAP,
        write_relays: &WRITE_RELAYS,
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
