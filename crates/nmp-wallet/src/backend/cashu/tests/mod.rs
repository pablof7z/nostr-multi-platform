//! Unit tests for the Cashu [`super::CashuWalletBackend`] adapter (#2895 W2).
//!
//! Split into cohesive submodules (AGENTS.md LOC discipline); this file owns
//! the shared test harness (capability adapters, context builders, and a
//! minimal local-socket mint mock) each submodule reaches through `use
//! super::*`.
//!
//! - [`create_wallet_tests`] — `CreateCashuWallet`: fail-closed (no account,
//!   unsupported mint), the happy-path encrypt->sign->publish chain, and the
//!   signer-can't-NIP-44 fail-closed branch.
//! - [`deposit_tests`] — `DepositQuoteCashu`/`CompleteDepositCashu`: the "journals
//!   before the mint request" ordering invariant, the auto-settle path
//!   against a mockable mint, fail-closed unsupported-mint/unknown-quote, and
//!   `dispatch_token_event`'s ledger/journal wiring with synthetic proofs.
//! - [`snapshot_tests`] — the bounded projection never carries a secret/quote
//!   id, mirroring `projection.rs`'s own redaction test.
//! - [`reset_tests`] — `CashuWalletBackend::reset` (epic #2864 Wave C,
//!   #2908): the cross-account data-leak fix clears created/mints/pubkey/
//!   pending-operations/balances back to a fresh, never-created wallet.

use std::sync::Mutex;

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    ActionStageTracker, CachedEventLookup, EmptyDmInboxRelayLookup, KernelClock, KernelEvent,
    NoopErrorSurface, NoopHostOpHandlerAccess, NoopLocalSignerAccess, NoopWalletKernelAccess,
    NoopZapProfileLookup, ProtocolCommand, ProtocolCommandContext, ProtocolCommandContextParts,
    RecipientRelayLookup,
};

use super::*;

mod create_wallet_tests;
mod deposit_retry_tests;
mod deposit_tests;
mod publish_info_tests;
mod redeem_tests;
mod reset_tests;
mod send_tests;
mod snapshot_tests;

/// Captures every `ActorCommand` a `run()` call sends synchronously via
/// `ctx.send`.
pub(super) struct Sink {
    pub(super) sends: Mutex<Vec<ActorCommand>>,
}

impl Sink {
    pub(super) fn new() -> Self {
        Self {
            sends: Mutex::new(Vec::new()),
        }
    }
}

pub(super) struct FixedClock(pub(super) u64);
impl KernelClock for FixedClock {
    fn now_secs(&self) -> u64 {
        self.0
    }
}

/// Fixed relay list for every recipient — a Cashu backend command always
/// resolves relays for its own account pubkey (self-publish).
pub(super) struct FixedRecipientLookup(pub(super) Vec<String>);
impl RecipientRelayLookup for FixedRecipientLookup {
    fn recipient_publish_relays(&self, _recipient: &str, _kind: u32) -> Vec<String> {
        self.0.clone()
    }
}

/// Build a `ProtocolCommandContext` with a REAL `command_sender` (backed by an
/// `mpsc::channel` the caller keeps the receiver for) so a command's
/// worker-thread `send`s are observable — mirrors
/// `nmp_nip57::lnurl::tests::ctx_with_sender`.
pub(super) fn ctx_with_sender<'a>(
    send: &'a dyn Fn(ActorCommand),
    command_sender: nmp_core::CommandSender,
    clock: &'a dyn KernelClock,
    recipients: &'a dyn RecipientRelayLookup,
) -> ProtocolCommandContext<'a> {
    static SIGNERS: NoopLocalSignerAccess = NoopLocalSignerAccess;
    static EMPTY_DM: EmptyDmInboxRelayLookup = EmptyDmInboxRelayLookup;
    static ERRORS: NoopErrorSurface = NoopErrorSurface;
    static HOST_OP: NoopHostOpHandlerAccess = NoopHostOpHandlerAccess;
    static WALLET: NoopWalletKernelAccess = NoopWalletKernelAccess;
    static ZAP: NoopZapProfileLookup = NoopZapProfileLookup;
    static STAGES: NoopStages = NoopStages;
    ProtocolCommandContext::new(ProtocolCommandContextParts {
        send,
        command_sender,
        clock,
        signers: &SIGNERS,
        dms: &EMPTY_DM,
        errors: &ERRORS,
        stages: &STAGES,
        recipients,
        host_op_handler: &HOST_OP,
        wallet_kernel: &WALLET,
        zap_profiles: &ZAP,
    })
}

pub(super) struct NoopStages;
impl ActionStageTracker for NoopStages {
    fn record_requested(&self, _correlation_id: &str) {}
}

/// #2917 — a fixed, in-memory [`CachedEventLookup`] double: tests seed
/// exactly the events a real kernel cache would have (or wouldn't), rather
/// than driving a real store.
#[derive(Default)]
pub(super) struct FixedCachedEvents(pub(super) Vec<KernelEvent>);
impl CachedEventLookup for FixedCachedEvents {
    fn event_by_id(&self, id: &str) -> Option<KernelEvent> {
        self.0.iter().find(|e| e.id == id).cloned()
    }
    fn latest_author_kind(&self, author: &str, kind: u32) -> Option<KernelEvent> {
        self.0
            .iter()
            .filter(|e| e.author == author && e.kind == kind)
            .max_by_key(|e| e.created_at)
            .cloned()
    }
}

/// [`ctx_with_sender`] plus the [`CachedEventLookup`] capability — the W8/W9
/// tests (`send_tests`/`redeem_tests`/`publish_info_tests`) need a recipient
/// or self kind:10019 (or a kind:9321) resolvable from the cache.
pub(super) fn ctx_with_cached_events<'a>(
    send: &'a dyn Fn(ActorCommand),
    command_sender: nmp_core::CommandSender,
    clock: &'a dyn KernelClock,
    recipients: &'a dyn RecipientRelayLookup,
    cached_events: &'a dyn CachedEventLookup,
) -> ProtocolCommandContext<'a> {
    ctx_with_sender(send, command_sender, clock, recipients).with_cached_events(cached_events)
}

/// [`ctx_with_cached_events`] plus a REAL (non-noop) [`ErrorSurface`] — the
/// #2917 commands' `run()`-body fail-closed branches (`super::fail`'s
/// `report_pre_dispatch_failure` call, e.g. `NO_CASHU_WALLET`/
/// `NO_TRUSTED_MINT`) report through the `ErrorSurface`/`ActionStageTracker`
/// capabilities (which write straight through to the kernel in production —
/// see `ErrorSurfaceAdapter`), NOT through `worker_tx`/`send`. Tests that
/// exercise one of THOSE branches need [`RecordingErrorSurface`] wired in to
/// observe it; tests exercising a worker-thread failure (`fail_worker`, after
/// `std::thread::spawn`) keep using `ctx_with_cached_events` and read
/// `worker_rx` as usual.
pub(super) fn ctx_with_cached_events_and_errors<'a>(
    send: &'a dyn Fn(ActorCommand),
    command_sender: nmp_core::CommandSender,
    clock: &'a dyn KernelClock,
    recipients: &'a dyn RecipientRelayLookup,
    cached_events: &'a dyn CachedEventLookup,
    errors: &'a dyn nmp_core::substrate::ErrorSurface,
) -> ProtocolCommandContext<'a> {
    static SIGNERS: NoopLocalSignerAccess = NoopLocalSignerAccess;
    static EMPTY_DM: EmptyDmInboxRelayLookup = EmptyDmInboxRelayLookup;
    static HOST_OP: NoopHostOpHandlerAccess = NoopHostOpHandlerAccess;
    static WALLET: NoopWalletKernelAccess = NoopWalletKernelAccess;
    static ZAP: NoopZapProfileLookup = NoopZapProfileLookup;
    static STAGES: NoopStages = NoopStages;
    ProtocolCommandContext::new(ProtocolCommandContextParts {
        send,
        command_sender,
        clock,
        signers: &SIGNERS,
        dms: &EMPTY_DM,
        errors,
        stages: &STAGES,
        recipients,
        host_op_handler: &HOST_OP,
        wallet_kernel: &WALLET,
        zap_profiles: &ZAP,
    })
    .with_cached_events(cached_events)
}

/// Captures what a `ProtocolCommandContext::set_last_error_token`/
/// `record_action_failure` call writes — the in-`run()` fail-closed reporting
/// path (see [`ctx_with_cached_events_and_errors`]'s doc comment).
#[derive(Default)]
pub(super) struct RecordingErrorSurface {
    pub(super) last_token_code: Mutex<Option<String>>,
    pub(super) failures: Mutex<Vec<(String, String)>>,
}

impl nmp_core::substrate::ErrorSurface for RecordingErrorSurface {
    fn set_last_error_toast(&self, _message: Option<String>) {}
    fn set_last_error_token(&self, token: &nmp_core::ui_token::UiToken) {
        *self.last_token_code.lock().unwrap() = Some(token.code().to_string());
    }
    fn record_action_failure(&self, correlation_id: String, reason: String) {
        self.failures.lock().unwrap().push((correlation_id, reason));
    }
}

/// Build a raw kind:10019 `KernelEvent` fixture by round-tripping
/// `nmp_nip60::nutzap::build_nutzap_info_event` through a throwaway sign —
/// the same conversion `nmp-core`'s own `gc_step_tests` uses to get from an
/// `EventBuilder` to hex-string wire fields, reused here so this fixture's
/// tags are byte-identical to what the real codec produces (never
/// hand-rolled tag rows that could silently drift from the real encoder).
pub(super) fn nutzap_info_kernel_event(
    author_pubkey_hex: &str,
    info: &nmp_nip60::nutzap::NutZapInfo,
    created_at: u64,
) -> KernelEvent {
    let keys = nostr::Keys::generate();
    let event = nmp_nip60::nutzap::build_nutzap_info_event(info, &keys)
        .expect("build nutzap info")
        .custom_created_at(nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(&keys)
        .expect("sign nutzap info fixture");
    KernelEvent {
        id: event.id.to_hex(),
        author: author_pubkey_hex.to_string(),
        kind: event.kind.as_u16() as u32,
        created_at: event.created_at.as_secs(),
        tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
        content: event.content.clone(),
        relay_provenance: Vec::new(),
    }
}

/// Same conversion as [`nutzap_info_kernel_event`], for a kind:9321 nutzap —
/// `sender_keys` signs it (so `event.author`/`.pubkey` line up), `p_tagged_to`
/// is the receiver this fixture claims to be for.
#[allow(clippy::too_many_arguments)]
pub(super) fn nutzap_kernel_event(
    sender_keys: &nostr::Keys,
    proofs: Vec<nmp_nip60::nutzap::NutZapProof>,
    mint_url: &str,
    p_tagged_to: &str,
    comment: Option<&str>,
    zapped_event_id: Option<&nostr::EventId>,
    created_at: u64,
) -> KernelEvent {
    let recipient = nostr::PublicKey::from_hex(p_tagged_to).expect("valid recipient pubkey hex");
    let event = nmp_nip60::nutzap::build_nutzap_event(proofs, mint_url, &recipient, comment, zapped_event_id)
        .expect("build nutzap event")
        .custom_created_at(nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(sender_keys)
        .expect("sign nutzap fixture");
    KernelEvent {
        id: event.id.to_hex(),
        author: sender_keys.public_key().to_hex(),
        kind: event.kind.as_u16() as u32,
        created_at: event.created_at.as_secs(),
        tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
        content: event.content.clone(),
        relay_provenance: Vec::new(),
    }
}

/// The mint every `backend_with_mint` wallet accepts.
pub(super) const MINT: &str = "https://testnut.cashu.space";

/// A `CashuWalletBackend` that already accepts [`MINT`] — the shared
/// precondition `DepositQuoteCashu`/`CompleteDepositCashu` tests need before they can
/// exercise the deposit flow at all.
pub(super) fn backend_with_mint() -> CashuWalletBackend {
    let backend = CashuWalletBackend::new();
    lock_state(&backend.state).mints = vec![MINT.to_string()];
    backend
}

/// A minimal Cashu `Proof` for tests that drive post-mint wiring directly
/// (never a real mint round-trip) — `secret` is a placeholder, never a real
/// spending secret.
pub(super) fn synthetic_proof(amount: u64, c: &str) -> nmp_nip60::cashu::types::Proof {
    nmp_nip60::cashu::types::Proof {
        amount,
        id: "keyset-1".to_string(),
        secret: "secret-never-logged".to_string(),
        c: c.to_string(),
        dleq: None,
        witness: None,
    }
}

/// Unwrap the `ActorMail::Command` variant a worker-thread `CommandSender`
/// send produces — the existing assertions only care about the `ActorCommand`.
pub(super) fn unwrap_mail(mail: nmp_core::ActorMail) -> ActorCommand {
    match mail {
        nmp_core::ActorMail::Command(cmd) => cmd,
        other => panic!("expected ActorMail::Command, got {other:?}"),
    }
}

/// Block (no polling — D8) for the next command on `rx`, panicking after a
/// bounded wait so a wiring regression fails the test instead of hanging CI.
pub(super) fn recv_command(rx: &std::sync::mpsc::Receiver<nmp_core::ActorMail>) -> ActorCommand {
    unwrap_mail(
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .expect("expected a command on the worker channel"),
    )
}

/// Minimal one-request-per-connection local HTTP/1.1 mock. Serves
/// `responses` in order, one per accepted TCP connection, always closing
/// after each response (`Connection: close`) so the client (ureq) cannot
/// keep a connection alive across responses — this makes accept-per-response
/// ordering deterministic regardless of ureq's own pooling behaviour.
/// Mirrors `nmp_blossom::upload::http`'s test-only mock server.
pub(super) fn spawn_mock_mint(responses: Vec<(u16, String)>) -> String {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock mint listener");
    let addr = listener.local_addr().expect("local_addr");
    let url = format!("http://{addr}");
    std::thread::spawn(move || {
        for (status, body) in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            let mut header_end = None;
            let mut content_length = 0usize;
            loop {
                let n = stream.read(&mut tmp).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if header_end.is_none() {
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = Some(pos + 4);
                        let headers = String::from_utf8_lossy(&buf[..pos]).to_string();
                        for line in headers.lines() {
                            if let Some(v) =
                                line.to_ascii_lowercase().strip_prefix("content-length:")
                            {
                                content_length = v.trim().parse().unwrap_or(0);
                            }
                        }
                    }
                }
                if let Some(he) = header_end {
                    if buf.len() >= he + content_length {
                        break;
                    }
                }
            }
            let reason = match status {
                200 => "OK",
                404 => "Not Found",
                _ => "Error",
            };
            let resp = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    url
}
