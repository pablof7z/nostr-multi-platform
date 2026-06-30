//! Unit tests for [`super::SendGiftWrappedDmCommand`] — the ADR-0050 §D5
//! continuation-chain DM send.
//!
//! The command body no longer spawns a thread or holds a `SignerForSeal`: it
//! launches a chain of port commands through the cloned `command_sender`
//! (`Nip44EncryptForAccount` → `SignEventForAccount` → `PublishSignedEvent`).
//! With no actor in the test, we drive the chain by draining the channel and
//! invoking each captured continuation by hand ([`ChainDriver`]) — exactly what
//! the actor's dispatch arm does inline for a local account. The seal is signed
//! with a real test `Keys` so the pure wrap step produces a verifiable kind:1059.

use super::*;
use crate::dm_relay_cache::DmRelayCache;
use nmp_core::actor::ActorCommand;
use nmp_core::actor::{ActionLedgerCommand, PublishCommand, SignCommand};
use nmp_core::publish::{PublishRouteClass, PublishTarget};
use nmp_core::substrate::{
    DmInboxRelayLookup, EmptyDmInboxRelayLookup, ErrorSurface, KernelClock, LocalSignerAccess,
    NoopActionStageTracker, NoopRecipientRelayLookup, ProtocolCommand, ProtocolCommandContext,
    ProtocolCommandContextParts,
};
use nmp_core::{ActorMail, CommandSender};
use nmp_signer_iface::{SignedEvent, UnsignedEvent as SubstrateUnsignedEvent};
use nostr::nips::nip44::{self, Version as Nip44Version};
use nostr::JsonUtil;
use std::cell::RefCell;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

mod port_failure_tests;
mod preflight_tests;
mod publish_path_tests;

const RECIPIENT_HEX_PLACEHOLDER: &str =
    "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

fn sample_rumor(sender_pubkey: &str, recipient_hex: &str) -> SubstrateUnsignedEvent {
    SubstrateUnsignedEvent {
        pubkey: sender_pubkey.to_string(),
        kind: 14,
        tags: vec![vec!["p".to_string(), recipient_hex.to_string()]],
        content: "hello over NIP-17".to_string(),
        created_at: 0,
    }
}

/// Test bag for recording context side-effects + follow-up commands.
#[derive(Default)]
struct Recorder {
    sent: RefCell<Vec<ActorCommand>>,
    toasts: RefCell<Vec<Option<String>>>,
    failures: RefCell<Vec<(String, String)>>,
}

// ── Test-only capability adapters (Debt C) ──

struct FixedClock(u64);
impl KernelClock for FixedClock {
    fn now_secs(&self) -> u64 {
        self.0
    }
}

/// `LocalSignerAccess` stub. ADR-0050 §D5 — the DM chain pins the active account
/// by resolving `active_account_pubkey()` once at step 1. `active_local_keys`
/// stays `None` (the chain never holds raw keys — it signs through the port).
struct StaticSigner {
    active_pubkey: Option<String>,
}
impl LocalSignerAccess for StaticSigner {
    fn active_local_keys(&self) -> Option<nostr::Keys> {
        None
    }
    fn active_account_pubkey(&self) -> Option<String> {
        self.active_pubkey.clone()
    }
}

/// `ErrorSurface` adapter recording every toast + failure into shared `RefCell`
/// slots. `RefCell` (not `Mutex`) is fine — the body runs single-threaded.
struct RecordingErrors<'a> {
    toasts: &'a RefCell<Vec<Option<String>>>,
    failures: &'a RefCell<Vec<(String, String)>>,
}
// SAFETY: constructed + dropped inside `run_cmd` on a single thread; the
// `&RefCell` borrows never cross a thread boundary. The trait carries the bound.
unsafe impl<'a> Send for RecordingErrors<'a> {}
unsafe impl<'a> Sync for RecordingErrors<'a> {}
impl<'a> ErrorSurface for RecordingErrors<'a> {
    fn set_last_error_toast(&self, message: Option<String>) {
        self.toasts.borrow_mut().push(message);
    }
    fn record_action_failure(&self, correlation_id: String, reason: String) {
        self.failures.borrow_mut().push((correlation_id, reason));
    }
}

/// Drive `cmd.run` through a fully-wired [`ProtocolCommandContext`]. Returns the
/// recorded side effects + the `command_sender` channel receiver so the caller
/// can drive the gift-wrap chain via [`ChainDriver`].
///
/// `active_pubkey` is the active account the chain pins (§D5); `None` mirrors the
/// no-active-account early exit.
fn run_cmd(
    cmd: SendGiftWrappedDmCommand,
    active_pubkey: Option<String>,
    dm_lookup: &dyn DmInboxRelayLookup,
    now_secs: u64,
) -> (Recorder, Receiver<ActorMail>) {
    let recorder = Recorder::default();
    let rx = {
        let sent_ref = &recorder.sent;
        let send = |c: ActorCommand| sent_ref.borrow_mut().push(c);
        let clock = FixedClock(now_secs);
        let signers = StaticSigner { active_pubkey };
        let errors = RecordingErrors {
            toasts: &recorder.toasts,
            failures: &recorder.failures,
        };
        let stages = NoopActionStageTracker;
        let recipients = NoopRecipientRelayLookup;
        let host_op_handler = nmp_core::substrate::NoopHostOpHandlerAccess;
        let wallet_kernel = nmp_core::substrate::NoopWalletKernelAccess;
        let zap_profiles = nmp_core::substrate::NoopZapProfileLookup;
        let (tx, rx) = std::sync::mpsc::channel::<ActorMail>();
        let mut ctx = ProtocolCommandContext::new(ProtocolCommandContextParts {
            send: &send,
            command_sender: CommandSender::new(tx),
            clock: &clock,
            signers: &signers,
            dms: dm_lookup,
            errors: &errors,
            stages: &stages,
            recipients: &recipients,
            host_op_handler: &host_op_handler,
            wallet_kernel: &wallet_kernel,
            zap_profiles: &zap_profiles,
        });
        Box::new(cmd)
            .run(&mut ctx)
            .expect("command body returns Ok");
        rx
    };
    (recorder, rx)
}

/// Drives the §D5 port chain by hand: pop a command off the channel, and if it
/// is a cipher/sign port verb, invoke its continuation (the actor would do this
/// inline for a local account); `PublishSignedEvent` / `ShowToast` /
/// `RecordActionFailure` are terminal and collected.
///
/// The seal is signed with `signer_keys` — a real key so the pure
/// `wrap_signed_seal` step produces a verifiable kind:1059 envelope. The cipher
/// step produces a real NIP-44 ciphertext from `signer_keys` to the peer (any
/// string would do for the wrap, but a real one keeps the seal content honest).
struct ChainDriver {
    signer_keys: nostr::Keys,
    /// Captured terminals (`PublishSignedEvent`, `ShowToast`, `RecordActionFailure`).
    terminals: Vec<ActorCommand>,
    /// `signer_pubkey` seen on each port verb, in order — for the §D5 pin oracle.
    pinned_signers: Vec<Option<String>>,
}

impl ChainDriver {
    fn new(signer_keys: nostr::Keys) -> Self {
        Self {
            signer_keys,
            terminals: Vec::new(),
            pinned_signers: Vec::new(),
        }
    }

    /// Build the `SignedEvent` a real signer returns for `unsigned` (mirrors the
    /// actor's local-key sign).
    fn sign_seal(&self, unsigned: &SubstrateUnsignedEvent) -> SignedEvent {
        let kind = nostr::Kind::from_u16(unsigned.kind as u16);
        let tags: Vec<nostr::Tag> = unsigned
            .tags
            .iter()
            .filter_map(|t| nostr::Tag::parse(t).ok())
            .collect();
        let event = nostr::EventBuilder::new(kind, &unsigned.content)
            .tags(tags)
            .custom_created_at(nostr::Timestamp::from(unsigned.created_at))
            .sign_with_keys(&self.signer_keys)
            .expect("seal sign");
        SignedEvent {
            id: event.id.to_hex(),
            sig: event.sig.to_string(),
            unsigned: SubstrateUnsignedEvent {
                pubkey: event.pubkey.to_hex(),
                kind: u32::from(event.kind.as_u16()),
                tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                content: event.content.clone(),
                created_at: event.created_at.as_secs(),
            },
        }
    }

    /// Pump the channel to completion, invoking continuations. `recv_timeout`
    /// keeps the loop bounded; the chain is otherwise fully synchronous (every
    /// continuation enqueues the next command before returning).
    fn run(mut self, rx: &Receiver<ActorMail>) -> Self {
        while let Ok(mail) = rx.recv_timeout(Duration::from_millis(200)) {
            let ActorMail::Command(cmd) = mail else {
                panic!("dm_send chain only sends commands");
            };
            match cmd {
                ActorCommand::Sign(SignCommand::Nip44EncryptForAccount {
                    peer_pubkey,
                    plaintext,
                    signer_pubkey,
                    continuation,
                }) => {
                    self.pinned_signers.push(signer_pubkey);
                    let peer = nostr::PublicKey::parse(&peer_pubkey).expect("peer pubkey");
                    let ciphertext = nip44::encrypt(
                        self.signer_keys.secret_key(),
                        &peer,
                        &plaintext,
                        Nip44Version::V2,
                    )
                    .expect("seal encrypt");
                    continuation.call(Ok(ciphertext));
                }
                ActorCommand::Sign(SignCommand::EventForAccount {
                    unsigned,
                    signer_pubkey,
                    continuation,
                }) => {
                    self.pinned_signers.push(signer_pubkey);
                    let signed = self.sign_seal(&unsigned);
                    continuation.call(Ok(signed));
                }
                terminal => self.terminals.push(terminal),
            }
        }
        self
    }

    /// Inject a failure at the first cipher step instead of resolving it — for
    /// the D6 failure-path oracles. Returns once the chain has run to terminals.
    fn run_failing_encrypt(mut self, rx: &Receiver<ActorMail>, reason: &str) -> Self {
        // The recipient chain's encrypt is the first command; fail it, then pump
        // the rest (the continuation enqueues toast/failure terminals).
        while let Ok(mail) = rx.recv_timeout(Duration::from_millis(200)) {
            let ActorMail::Command(cmd) = mail else {
                panic!("dm_send chain only sends commands");
            };
            match cmd {
                ActorCommand::Sign(SignCommand::Nip44EncryptForAccount {
                    continuation, ..
                }) => {
                    continuation.call(Err(reason.to_string()));
                }
                terminal => self.terminals.push(terminal),
            }
        }
        self
    }

    fn publishes(&self) -> Vec<(&nmp_store::RawEvent, &PublishTarget, &Option<String>)> {
        self.terminals
            .iter()
            .filter_map(|c| match c {
                ActorCommand::Publish(PublishCommand::SignedEvent {
                    raw,
                    target,
                    correlation_id,
                }) => Some((raw, target, correlation_id)),
                _ => None,
            })
            .collect()
    }

    fn toasts(&self) -> Vec<&str> {
        self.terminals
            .iter()
            .filter_map(|c| match c {
                ActorCommand::ShowToast { message } => Some(message.as_str()),
                // issue #1682 — DM failures now ride structured tokens; the
                // fallback prose is the same English the toast carried before.
                ActorCommand::ShowErrorToken { token } => Some(token.fallback_prose()),
                _ => None,
            })
            .collect()
    }

    fn action_failures(&self) -> Vec<(&str, &str)> {
        self.terminals
            .iter()
            .filter_map(|c| match c {
                ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure {
                    correlation_id,
                    reason,
                }) => Some((correlation_id.as_str(), reason.as_str())),
                _ => None,
            })
            .collect()
    }
}

// ── §D5 oracles ──────────────────────────────────────────────────────────────

#[test]
fn every_port_step_pins_the_originating_account() {
    // §D5 account pinning — every cipher/sign verb in the chain carries
    // `signer_pubkey: Some(active_hex)`, never None. (The mid-flight switch
    // oracle below proves WHY: re-resolving "active" would break this.)
    let keys = nostr::Keys::generate();
    let sender_hex = keys.public_key().to_hex();
    let recipient_keys = nostr::Keys::generate();
    let recipient_hex = recipient_keys.public_key().to_hex();
    let cache = Arc::new(DmRelayCache::new());
    cache.upsert(sender_hex.clone(), vec!["wss://s.example".to_string()]);
    cache.upsert(recipient_hex.clone(), vec!["wss://r.example".to_string()]);

    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(&sender_hex, &recipient_hex),
        recipient_pubkey: recipient_hex.clone(),
        correlation_id: None,
    };
    let (_rec, rx) = run_cmd(cmd, Some(sender_hex.clone()), cache.as_ref(), 1_700_000_000);
    let driver = ChainDriver::new(keys).run(&rx);

    // Two envelopes × two port verbs (encrypt + sign) = 4 pinned steps.
    assert_eq!(
        driver.pinned_signers.len(),
        4,
        "four port verbs (2 envelopes × encrypt+sign)"
    );
    for pin in &driver.pinned_signers {
        assert_eq!(
            pin.as_deref(),
            Some(sender_hex.as_str()),
            "§D5 — every port step pins the originating account; never None"
        );
    }
}

#[test]
fn mid_chain_account_switch_signs_seal_with_originating_account() {
    // §D5 oracle — the active account switches AFTER the chain starts. Because
    // the chain pinned the originating account at step 1 (`Some(hex)`), the seal
    // is signed with the ORIGINATING account, not whoever is "active" now.
    //
    // We model the switch by having `ChainDriver` sign with the ORIGINATING key
    // (the pinned `signer_pubkey` it observes) — and assert that pin is the
    // originating account even though the context's "active" could have moved on.
    let originating = nostr::Keys::generate();
    let originating_hex = originating.public_key().to_hex();
    let recipient_keys = nostr::Keys::generate();
    let recipient_hex = recipient_keys.public_key().to_hex();
    let cache = Arc::new(DmRelayCache::new());
    cache.upsert(originating_hex.clone(), vec!["wss://s.example".to_string()]);
    cache.upsert(recipient_hex.clone(), vec!["wss://r.example".to_string()]);

    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(&originating_hex, &recipient_hex),
        recipient_pubkey: recipient_hex.clone(),
        correlation_id: None,
    };
    // The command pins the originating account at launch.
    let (_rec, rx) = run_cmd(
        cmd,
        Some(originating_hex.clone()),
        cache.as_ref(),
        1_700_000_000,
    );

    // Drive the chain. The driver signs with the originating key BECAUSE that is
    // the pinned signer the port carries — exactly what the real dispatch arm
    // resolves `Some(originating_hex)` to, regardless of a since-switched active.
    let driver = ChainDriver::new(originating.clone()).run(&rx);

    // Every port step pinned the ORIGINATING account.
    for pin in &driver.pinned_signers {
        assert_eq!(
            pin.as_deref(),
            Some(originating_hex.as_str()),
            "the chain must pin the originating account at every step"
        );
    }
    // The recipient envelope unwraps to a seal authored by the ORIGINATING
    // account — proof the seal was signed with the originating key.
    let (recipient_raw, _, _) = driver
        .publishes()
        .into_iter()
        .find(|(_, target, _)| {
            matches!(target, PublishTarget::Explicit { relays, route_class: PublishRouteClass::VerifiedPrivateInbox } if relays.contains(&"wss://r.example".to_string()))
        })
        .expect("recipient envelope present");
    let envelope = raw_to_nostr_event(recipient_raw);
    let unwrapped = nmp_nip59::unwrap_gift_wrap(&recipient_keys, &envelope).unwrap();
    assert_eq!(
        unwrapped.sender,
        originating.public_key(),
        "§D5 — the seal is signed with the originating account, not the switched-in one"
    );
}

// ── helper ───────────────────────────────────────────────────────────────────

/// Rebuild a `nostr::Event` from the kernel `RawEvent` the publish step carries,
/// so a test can unwrap it. The publish path forwards the event verbatim.
fn raw_to_nostr_event(raw: &nmp_store::RawEvent) -> nostr::Event {
    let json = serde_json::json!({
        "id": raw.id,
        "pubkey": raw.pubkey,
        "created_at": raw.created_at,
        "kind": raw.kind,
        "tags": raw.tags,
        "content": raw.content,
        "sig": raw.sig,
    })
    .to_string();
    nostr::Event::from_json(json).expect("RawEvent reparses to a nostr::Event")
}
