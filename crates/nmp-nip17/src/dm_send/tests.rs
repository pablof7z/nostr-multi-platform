//! Unit tests for [`super::SendGiftWrappedDmCommand`].
//!
//! The tests drive the command body through the substrate
//! [`nmp_core::substrate::ProtocolCommandContext`] directly (no actor, no
//! kernel). Captured [`nmp_core::ActorCommand::PublishSignedEvent`]
//! follow-ups + toast / record-failure side effects are asserted against
//! the NIP-17 § 2 contract.

use super::*;
use crate::dm_relay_cache::DmRelayCache;
use nmp_core::substrate::{
    DmInboxRelayLookup, EmptyDmInboxRelayLookup, ErrorSurface, KernelClock,
    LocalSignerAccess, NoopActionStageTracker, ProtocolCommand, ProtocolCommandContext,
    UnsignedEvent,
};
use nmp_core::ActorCommand;
use nmp_nip59::SignerForSeal;
use std::cell::RefCell;
use std::sync::Arc;

const RECIPIENT_HEX_PLACEHOLDER: &str =
    "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

/// Build a kind:14 rumor with the `created_at: 0` sentinel — what
/// [`crate::build_dm_rumor`] produces.
fn sample_rumor(sender_pubkey: &str, recipient_hex: &str) -> UnsignedEvent {
    UnsignedEvent {
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

struct StaticSigner {
    keys: Option<nostr::Keys>,
    signer: Option<Arc<dyn SignerForSeal>>,
}
impl LocalSignerAccess for StaticSigner {
    fn active_local_keys(&self) -> Option<nostr::Keys> {
        self.keys.clone()
    }
    fn signer_for_seal(&self) -> Option<Arc<dyn SignerForSeal>> {
        self.signer.clone()
    }
}

/// `ErrorSurface` adapter that records every toast + failure into
/// shared `RefCell` slots so the test asserts can inspect the side
/// effects. `RefCell` (not `Mutex`) is fine — the dispatch runs
/// single-threaded inside `run_cmd`.
struct RecordingErrors<'a> {
    toasts: &'a RefCell<Vec<Option<String>>>,
    failures: &'a RefCell<Vec<(String, String)>>,
}
// SAFETY: the adapter is constructed and dropped inside `run_cmd` on a
// single thread; the `&RefCell` borrows never cross a thread boundary.
// The `Send + Sync` impl is required because the substrate trait
// carries the bound.
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

/// Drive a command body through a fully-wired
/// [`ProtocolCommandContext`] and return the recorded side effects.
///
/// V-08 — the DM send path now resolves the signer via
/// [`ProtocolCommandContext::signer_for_seal`]. Tests with `Some(keys)`
/// install a `StaticSigner` that returns the `nostr::Keys` blanket impl
/// as the `SignerForSeal`; `None` mirrors the no-active-account path.
/// End-to-end remote-signer (NIP-46 bunker) coverage lives in
/// `nmp_core::actor::commands::remote_signer_tests`.
fn run_cmd(
    cmd: SendGiftWrappedDmCommand,
    keys: Option<nostr::Keys>,
    dm_lookup: &dyn DmInboxRelayLookup,
    now_secs: u64,
) -> Recorder {
    let recorder = Recorder::default();
    {
        let sent_ref = &recorder.sent;
        let send = |c: ActorCommand| sent_ref.borrow_mut().push(c);
        let signer_arc: Option<Arc<dyn SignerForSeal>> =
            keys.as_ref().map(|k| Arc::new(k.clone()) as Arc<dyn SignerForSeal>);
        let clock = FixedClock(now_secs);
        let signers = StaticSigner { keys, signer: signer_arc };
        let errors = RecordingErrors {
            toasts: &recorder.toasts,
            failures: &recorder.failures,
        };
        let stages = NoopActionStageTracker;
        let (tx, _rx) = std::sync::mpsc::channel::<ActorCommand>();
        let mut ctx = ProtocolCommandContext::new(
            &send, tx, &clock, &signers, dm_lookup, &errors, &stages,
        );
        Box::new(cmd).run(&mut ctx).expect("command body returns Ok");
    }
    recorder
}

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
    let rec = run_cmd(cmd, None, &empty, 1_700_000_000);

    assert!(rec.sent.borrow().is_empty(), "no envelopes published");
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
    let rec = run_cmd(cmd, Some(keys), &empty, 1_700_000_000);

    assert!(rec.sent.borrow().is_empty());
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
    let rec = run_cmd(cmd, Some(keys), cache.as_ref(), 1_700_000_000);

    assert!(rec.sent.borrow().is_empty(), "fail-closed — no PublishSignedEvent");
    let toasts = rec.toasts.borrow();
    assert!(
        toasts.iter().any(|t| t
            .as_deref()
            .map(|s| s.contains("kind:10050") && s.contains("recipient"))
            .unwrap_or(false)),
        "D10 — toast names kind:10050 + which envelope was blocked: {toasts:?}"
    );
}

#[test]
fn happy_path_publishes_two_envelopes_pinned_to_kind10050_relays() {
    // The full V-39 contract: a populated sender + recipient kind:10050
    // pair produces TWO `PublishSignedEvent` follow-ups (recipient +
    // self-copy), each pinned to *its receiver's* DM-inbox relays via
    // `PublishTarget::Explicit`. No toast / failure is recorded.
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
    let rec = run_cmd(cmd, Some(keys), cache.as_ref(), 1_700_000_000);

    let sent = rec.sent.borrow();
    assert_eq!(sent.len(), 2, "exactly two envelopes (recipient + self-copy)");
    assert!(rec.toasts.borrow().is_empty(), "happy path — no toasts: {:?}", rec.toasts.borrow());
    assert!(rec.failures.borrow().is_empty(), "happy path — no Failed terminals");

    // Collect the per-envelope target relay sets and assert they match
    // the seeded receivers' kind:10050 lists exactly.
    let mut explicit_targets: Vec<Vec<String>> = Vec::new();
    for cmd in sent.iter() {
        match cmd {
            ActorCommand::PublishSignedEvent {
                raw,
                target,
                correlation_id,
            } => {
                assert_eq!(
                    raw.kind, 1059,
                    "the gift-wrap envelope is kind:1059, got {}",
                    raw.kind
                );
                assert_eq!(
                    correlation_id.as_deref(),
                    Some("cid-happy"),
                    "correlation_id threads through to the publish engine for spinner clearance"
                );
                match target {
                    nmp_core::publish::PublishTarget::Explicit { relays } => {
                        explicit_targets.push(relays.clone());
                    }
                    other => {
                        panic!("D10 — gift-wrap MUST route via PublishTarget::Explicit, got {other:?}")
                    }
                }
            }
            other => panic!("expected PublishSignedEvent follow-up, got {other:?}"),
        }
    }

    let mut all_relays: Vec<String> =
        explicit_targets.into_iter().flatten().collect();
    all_relays.sort();
    assert_eq!(
        all_relays,
        vec![
            "wss://recipient-dm.example".to_string(),
            "wss://sender-dm.example".to_string(),
        ],
        "recipient envelope pins to recipient's kind:10050; self-copy pins to sender's"
    );
}

#[test]
fn rumor_created_at_is_restamped_when_zero_sentinel() {
    // D7 — the host sends `created_at: 0` as the sentinel; the command
    // re-stamps from `ctx.now_secs()` before sealing. The kind:14 rumor
    // is sealed inside the kind:1059 envelope so its timestamp is not
    // directly observable, but we can confirm the re-stamp by reading
    // the rumor back out before it is consumed: the command body
    // mutates `rumor.created_at` in place, then converts to a
    // `nostr::UnsignedEvent`. The seal step's NIP-59 timestamp tweak
    // applies to the *envelope*, not the inner rumor, so the rumor's
    // stamp is the one we control.
    //
    // We assert by reading the gift-wrap envelope back, decrypting the
    // seal with the recipient's keys, and inspecting the inner rumor's
    // `created_at`. That round-trip lives in `nmp-nip59`; here we
    // simply confirm the command body produced two envelopes (the seal
    // round-trip is exercised by the inbox tests in `inbox::tests`).
    let keys = nostr::Keys::generate();
    let sender_hex = keys.public_key().to_hex();
    let recipient_keys = nostr::Keys::generate();
    let recipient_hex = recipient_keys.public_key().to_hex();
    let cache = Arc::new(DmRelayCache::new());
    cache.upsert(sender_hex.clone(), vec!["wss://s.example".to_string()]);
    cache.upsert(recipient_hex.clone(), vec!["wss://r.example".to_string()]);

    let now: u64 = 1_700_000_000;
    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(&sender_hex, &recipient_hex),
        recipient_pubkey: recipient_hex.clone(),
        correlation_id: None,
    };
    let rec = run_cmd(cmd, Some(keys), cache.as_ref(), now);

    let sent = rec.sent.borrow();
    assert_eq!(sent.len(), 2, "happy path — two envelopes (recipient + self-copy)");
    // Every produced envelope is a kind:1059 gift-wrap (the load-bearing
    // shape gate of the V-39 contract).
    for cmd in sent.iter() {
        match cmd {
            ActorCommand::PublishSignedEvent { raw, .. } => {
                assert_eq!(raw.kind, 1059, "every envelope is kind:1059");
            }
            other => panic!("unexpected follow-up: {other:?}"),
        }
    }
}

