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
//! - [`deposit_tests`] — `DepositQuote`/`CompleteDeposit`: the "journals
//!   before the mint request" ordering invariant, the auto-settle path
//!   against a mockable mint, fail-closed unsupported-mint/unknown-quote, and
//!   `dispatch_token_event`'s ledger/journal wiring with synthetic proofs.
//! - [`snapshot_tests`] — the bounded projection never carries a secret/quote
//!   id, mirroring `projection.rs`'s own redaction test.

use std::sync::Mutex;

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    ActionStageTracker, EmptyDmInboxRelayLookup, KernelClock, NoopErrorSurface,
    NoopHostOpHandlerAccess, NoopLocalSignerAccess, NoopWalletKernelAccess, NoopZapProfileLookup,
    ProtocolCommandContext, ProtocolCommandContextParts, RecipientRelayLookup,
};

use super::*;

mod create_wallet_tests;
mod deposit_tests;
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
                            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
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
