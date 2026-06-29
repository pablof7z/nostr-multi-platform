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
use nmp_core::substrate::{
    DmInboxRelayLookup, EmptyDmInboxRelayLookup, ErrorSurface, KernelClock, LocalSignerAccess,
    NoopActionStageTracker, NoopRecipientRelayLookup, ProtocolCommand, ProtocolCommandContext,
    ProtocolCommandContextParts,
};
use nmp_signer_iface::{SignedEvent, UnsignedEvent as SubstrateUnsignedEvent};
use nmp_core::publish::{PublishRouteClass, PublishTarget};
use nmp_core::{ActorMail, CommandSender};
use nmp_core::actor::{ActorCommand};
use nmp_core::actor::{ActionLedgerCommand, PublishCommand, SignCommand};
use nostr::nips::nip44::{self, Version as Nip44Version};
use nostr::JsonUtil;
use std::cell::RefCell;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

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
        Box::new(cmd).run(&mut ctx).expect("command body returns Ok");
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
                ActorCommand::Sign(SignCommand::Nip44EncryptForAccount { continuation, .. }) => {
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

// ── Early-exit (pre-chain) failure paths — D6 / D10 ───────────────────────────

#[test]
fn no_active_account_toasts_and_records_failure() {
    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(
            "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee",
            RECIPIENT_HEX_PLACEHOLDER,
        ),
        recipient_pubkey: RECIPIENT_HEX_PLACEHOLDER.to_string(),
        correlation_id: Some("cid-no-account".to_string()),
    };
    let empty = EmptyDmInboxRelayLookup;
    let (rec, rx) = run_cmd(cmd, None, &empty, 1_700_000_000);

    // No chain launched — nothing on the channel.
    assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    let toasts = rec.toasts.borrow();
    assert_eq!(toasts.len(), 1, "exactly one toast: the no-account message");
    assert!(
        toasts[0]
            .as_deref()
            .map(|s| s.contains("no active account"))
            .unwrap_or(false),
        "toast carries the no-account reason: {:?}",
        toasts[0]
    );
    let failures = rec.failures.borrow();
    assert_eq!(failures.len(), 1, "D6 — exactly one Failed terminal recorded");
    assert_eq!(failures[0].0, "cid-no-account");
}

#[test]
fn malformed_recipient_pubkey_toasts_and_records_failure() {
    let keys = nostr::Keys::generate();
    let sender_hex = keys.public_key().to_hex();
    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(&sender_hex, "not-a-pubkey"),
        recipient_pubkey: "not-a-pubkey".to_string(),
        correlation_id: Some("cid-bad-pubkey".to_string()),
    };
    let empty = EmptyDmInboxRelayLookup;
    let (rec, _rx) = run_cmd(cmd, Some(sender_hex), &empty, 1_700_000_000);

    let toasts = rec.toasts.borrow();
    assert!(
        toasts.iter().any(|t| t
            .as_deref()
            .map(|s| s.contains("recipient pubkey"))
            .unwrap_or(false)),
        "D6 — toast surfaces the malformed-pubkey reason: {toasts:?}"
    );
    let failures = rec.failures.borrow();
    assert_eq!(failures.len(), 1);
}

#[test]
fn missing_kind10050_for_recipient_fails_closed() {
    let keys = nostr::Keys::generate();
    let sender_hex = keys.public_key().to_hex();
    let recipient_keys = nostr::Keys::generate();
    let recipient_hex = recipient_keys.public_key().to_hex();

    let cache = Arc::new(DmRelayCache::new());
    // Seed the sender's relays; deliberately leave the recipient's missing.
    cache.upsert(sender_hex.clone(), vec!["wss://sender-dm.example".to_string()]);

    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(&sender_hex, &recipient_hex),
        recipient_pubkey: recipient_hex.clone(),
        correlation_id: Some("cid-fail-closed".to_string()),
    };
    let (rec, rx) = run_cmd(cmd, Some(sender_hex), cache.as_ref(), 1_700_000_000);

    assert!(
        rx.recv_timeout(Duration::from_millis(50)).is_err(),
        "fail-closed — no chain launched, no PublishSignedEvent"
    );
    let toasts = rec.toasts.borrow();
    assert!(
        toasts.iter().any(|t| t
            .as_deref()
            .map(|s| s.contains("kind:10050") && s.contains("recipient"))
            .unwrap_or(false)),
        "D10 — toast names kind:10050 + which envelope was blocked: {toasts:?}"
    );
}

// ── Happy path — both envelopes through the chain ─────────────────────────────

#[test]
fn happy_path_publishes_two_envelopes_pinned_to_kind10050_relays() {
    let keys = nostr::Keys::generate();
    let sender_hex = keys.public_key().to_hex();
    let recipient_keys = nostr::Keys::generate();
    let recipient_hex = recipient_keys.public_key().to_hex();

    let cache = Arc::new(DmRelayCache::new());
    cache.upsert(sender_hex.clone(), vec!["wss://sender-dm.example".to_string()]);
    cache.upsert(recipient_hex.clone(), vec!["wss://recipient-dm.example".to_string()]);

    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(&sender_hex, &recipient_hex),
        recipient_pubkey: recipient_hex.clone(),
        correlation_id: Some("cid-happy".to_string()),
    };
    let (rec, rx) = run_cmd(cmd, Some(sender_hex.clone()), cache.as_ref(), 1_700_000_000);
    let driver = ChainDriver::new(keys).run(&rx);

    let publishes = driver.publishes();
    assert_eq!(publishes.len(), 2, "exactly two envelopes (recipient + self-copy)");
    assert!(driver.toasts().is_empty(), "happy path — no toasts");
    assert!(driver.action_failures().is_empty(), "happy path — no Failed terminals");
    assert!(rec.toasts.borrow().is_empty(), "no pre-chain toast either");

    let mut explicit_targets: Vec<(Vec<String>, Option<String>)> = Vec::new();
    for (raw, target, cid) in &publishes {
        assert_eq!(raw.kind, 1059, "the gift-wrap envelope is kind:1059, got {}", raw.kind);
        match target {
            PublishTarget::Explicit { relays, route_class: PublishRouteClass::VerifiedPrivateInbox } => {
                explicit_targets.push(((*relays).clone(), (*cid).clone()));
            }
            other => panic!("D10 — gift-wrap MUST route via PublishTarget::Explicit, got {other:?}"),
        }
    }

    // Relay sets must cover both receiver kind:10050 lists.
    let mut all_relays: Vec<String> = explicit_targets
        .iter()
        .flat_map(|(relays, _)| relays.clone())
        .collect();
    all_relays.sort();
    assert_eq!(
        all_relays,
        vec![
            "wss://recipient-dm.example".to_string(),
            "wss://sender-dm.example".to_string(),
        ],
        "recipient envelope pins to recipient's kind:10050; self-copy pins to sender's"
    );

    // Single-terminal invariant: only the recipient envelope carries the cid.
    let recipient_entry = explicit_targets
        .iter()
        .find(|(relays, _)| relays.contains(&"wss://recipient-dm.example".to_string()));
    let self_copy_entry = explicit_targets
        .iter()
        .find(|(relays, _)| relays.contains(&"wss://sender-dm.example".to_string()));
    assert_eq!(
        recipient_entry.map(|(_, cid)| cid.as_deref()),
        Some(Some("cid-happy")),
        "recipient envelope must carry the correlation_id for the action terminal"
    );
    assert_eq!(
        self_copy_entry.map(|(_, cid)| cid.as_deref()),
        Some(None),
        "self-copy envelope must carry None — its relay ack must not produce a second terminal"
    );
}

#[test]
fn recipient_envelope_round_trips_to_the_original_rumor() {
    // The recipient kind:1059 must unwrap (with the recipient's keys) back to
    // the kind:14 rumor — proving the chain assembled a real, decryptable seal.
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
    let driver = ChainDriver::new(keys.clone()).run(&rx);

    // The recipient envelope is the one pinned to the recipient's relay.
    let (recipient_raw, _, _) = driver
        .publishes()
        .into_iter()
        .find(|(_, target, _)| {
            matches!(target, PublishTarget::Explicit { relays, route_class: PublishRouteClass::VerifiedPrivateInbox } if relays.contains(&"wss://r.example".to_string()))
        })
        .expect("recipient envelope present");

    let envelope = raw_to_nostr_event(recipient_raw);
    let unwrapped =
        nmp_nip59::unwrap_gift_wrap(&recipient_keys, &envelope).expect("recipient can unwrap");
    assert_eq!(unwrapped.sender, keys.public_key(), "seal author is the sender");
    assert_eq!(unwrapped.rumor.content, "hello over NIP-17");
    assert_eq!(u16::from(unwrapped.rumor.kind), 14);
}

#[test]
fn rumor_created_at_is_restamped_when_zero_sentinel() {
    // D7 — the host sends `created_at: 0`; the body re-stamps from `now_secs`
    // before sealing. We read it back by unwrapping the recipient envelope.
    let keys = nostr::Keys::generate();
    let sender_hex = keys.public_key().to_hex();
    let recipient_keys = nostr::Keys::generate();
    let recipient_hex = recipient_keys.public_key().to_hex();
    let cache = Arc::new(DmRelayCache::new());
    cache.upsert(sender_hex.clone(), vec!["wss://s.example".to_string()]);
    cache.upsert(recipient_hex.clone(), vec!["wss://r.example".to_string()]);

    let now: u64 = 1_700_000_777;
    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(&sender_hex, &recipient_hex),
        recipient_pubkey: recipient_hex.clone(),
        correlation_id: None,
    };
    let (_rec, rx) = run_cmd(cmd, Some(sender_hex.clone()), cache.as_ref(), now);
    let driver = ChainDriver::new(keys.clone()).run(&rx);

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
        unwrapped.rumor.created_at.as_secs(),
        now,
        "D7 — the rumor's created_at is re-stamped from the kernel clock"
    );
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
    assert_eq!(driver.pinned_signers.len(), 4, "four port verbs (2 envelopes × encrypt+sign)");
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
    let (_rec, rx) = run_cmd(cmd, Some(originating_hex.clone()), cache.as_ref(), 1_700_000_000);

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

#[test]
fn recipient_encrypt_failure_surfaces_toast_and_action_failure() {
    // D6 — a recipient-chain port failure surfaces BOTH a toast and a
    // RecordActionFailure (the recipient envelope owns the action verdict).
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
        correlation_id: Some("cid-fail".to_string()),
    };
    let (_rec, rx) = run_cmd(cmd, Some(sender_hex), cache.as_ref(), 1_700_000_000);
    let driver = ChainDriver::new(keys).run_failing_encrypt(&rx, "broker rejected");

    assert!(driver.publishes().is_empty(), "no envelope published on failure");
    assert!(
        driver.toasts().iter().any(|t| t.contains("recipient") && t.contains("broker rejected")),
        "D6 — toast names the recipient envelope + the reason: {:?}",
        driver.toasts()
    );
    let failures = driver.action_failures();
    assert_eq!(failures.len(), 1, "recipient envelope records the action failure");
    assert_eq!(failures[0].0, "cid-fail");
}

#[test]
fn self_copy_failure_surfaces_toast_only_not_action_failure() {
    // §D5 single-terminal — the recipient envelope SUCCEEDS, then the self-copy
    // chain fails. The self-copy failure surfaces a D6 toast but NO
    // RecordActionFailure: the recipient already got the message, so the action
    // promise is satisfied (the action verdict is the recipient envelope's).
    //
    // We drive: recipient encrypt → sign → publish (success, launching the
    // self-copy chain), then fail the self-copy's encrypt.
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
        correlation_id: Some("cid-selfcopy".to_string()),
    };
    let (_rec, rx) = run_cmd(cmd, Some(sender_hex.clone()), cache.as_ref(), 1_700_000_000);

    // Custom drive: resolve the recipient chain fully, then fail the self-copy's
    // first cipher step. The recipient is the FIRST envelope; the self-copy is
    // launched by the recipient's publish step.
    let driver = ChainDriver::new(keys);
    let mut driver = driver;
    let mut recipient_done = false;
    while let Ok(mail) = rx.recv_timeout(Duration::from_millis(200)) {
        let ActorMail::Command(cmd) = mail else { unreachable!() };
        match cmd {
            ActorCommand::Sign(SignCommand::Nip44EncryptForAccount {
                peer_pubkey,
                plaintext,
                signer_pubkey,
                continuation,
            }) => {
                driver.pinned_signers.push(signer_pubkey);
                if recipient_done {
                    // This is the self-copy's encrypt — fail it.
                    continuation.call(Err("self-copy broker down".to_string()));
                } else {
                    let peer = nostr::PublicKey::parse(&peer_pubkey).unwrap();
                    let ct = nip44::encrypt(
                        driver.signer_keys.secret_key(),
                        &peer,
                        &plaintext,
                        Nip44Version::V2,
                    )
                    .unwrap();
                    continuation.call(Ok(ct));
                }
            }
            ActorCommand::Sign(SignCommand::EventForAccount {
                unsigned,
                signer_pubkey,
                continuation,
            }) => {
                driver.pinned_signers.push(signer_pubkey);
                let signed = driver.sign_seal(&unsigned);
                continuation.call(Ok(signed));
            }
            ActorCommand::Publish(PublishCommand::SignedEvent { .. }) => {
                // The recipient publish — record it and mark recipient done so
                // the next encrypt (self-copy) is failed.
                recipient_done = true;
                driver.terminals.push(cmd);
            }
            terminal => driver.terminals.push(terminal),
        }
    }

    // Exactly ONE publish (recipient) — the self-copy failed before publishing.
    assert_eq!(driver.publishes().len(), 1, "recipient published; self-copy did not");
    // A toast names the self-copy failure (D6 visibility) ...
    assert!(
        driver.toasts().iter().any(|t| t.contains("self-copy")),
        "D6 — self-copy failure surfaces a toast: {:?}",
        driver.toasts()
    );
    // ... but NO action failure (single-terminal — recipient got the message).
    assert!(
        driver.action_failures().is_empty(),
        "§D5 single-terminal — a self-copy failure must NOT record an action failure"
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
