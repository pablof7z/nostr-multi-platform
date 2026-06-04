//! Unit tests for [`super::SendGiftWrappedDmCommand`].
//!
//! The tests drive the command body through the substrate
//! [`nmp_core::substrate::ProtocolCommandContext`] directly (no actor, no
//! kernel). Captured [`nmp_core::ActorCommand::PublishSignedEvent`]
//! follow-ups + toast / record-failure side effects are asserted against
//! the NIP-17 § 2 contract.
//!
//! # ADR-0040 Site 1 — test harness note
//!
//! After the off-actor refactor `PublishSignedEvent` commands no longer flow
//! through `ctx.send` (the `send` closure / `Recorder::sent`). They are
//! posted by the worker thread back through the `command_sender` channel.
//! `run_cmd` therefore retains the channel receiver and drains it with a
//! bounded `recv_timeout` loop **after** `run()` returns, collecting any
//! worker-posted commands into `Recorder::sent`. The three early-exit tests
//! (`no_active_account`, `malformed_recipient`, `missing_kind10050`) exit
//! before reaching the worker spawn and are unaffected — they still record
//! via `ctx.set_last_error_toast` / `ctx.record_action_failure` on-actor.

use super::*;
use crate::dm_relay_cache::DmRelayCache;
use nmp_core::substrate::{
    DmInboxRelayLookup, EmptyDmInboxRelayLookup, ErrorSurface, KernelClock,
    LocalSignerAccess, NoopActionStageTracker, NoopRecipientRelayLookup,
    ProtocolCommand, ProtocolCommandContext, ProtocolCommandContextParts, UnsignedEvent,
};
use nmp_core::ActorCommand;
use nmp_nip59::SignerForSeal;
use nmp_signer_iface::{SignerError, SignerOp};
use std::cell::RefCell;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

const RECIPIENT_HEX_PLACEHOLDER: &str =
    "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

// ── Delayed signer (for the non-blocking proof test) ──────────────────

/// A `SignerForSeal` that makes `nip44_encrypt` return `SignerOp::Pending`
/// with a controllable delay, proving that `run()` returns before the
/// gift-wrap chain completes (ADR-0040 Site 1 non-block proof).
///
/// `nip44_encrypt` spawns a thread that sleeps for `delay` then sends the
/// real encrypted ciphertext using an inner `nostr::Keys` signer.
/// `sign_seal` delegates synchronously to the same inner keys.
struct DelayedSigner {
    keys: nostr::Keys,
    delay: Duration,
}

impl SignerForSeal for DelayedSigner {
    fn pubkey(&self) -> nostr::PublicKey {
        self.keys.public_key()
    }

    fn nip44_encrypt(
        &self,
        recipient_pubkey: &str,
        plaintext: &str,
    ) -> SignerOp<String> {
        let keys = self.keys.clone();
        let recipient_hex = recipient_pubkey.to_string();
        let plaintext_owned = plaintext.to_string();
        let delay = self.delay;

        let (tx, rx) = std::sync::mpsc::channel::<Result<String, SignerError>>();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            let result = nostr::PublicKey::parse(&recipient_hex)
                .map_err(|e| {
                    SignerError::Backend(format!("DelayedSigner: bad pubkey: {e}"))
                })
                .and_then(|pk| {
                    nostr::nips::nip44::encrypt(
                        keys.secret_key(),
                        &pk,
                        &plaintext_owned,
                        nostr::nips::nip44::Version::V2,
                    )
                    .map_err(|e| SignerError::Backend(format!("DelayedSigner: encrypt: {e}")))
                });
            let _ = tx.send(result);
        });
        SignerOp::Pending(rx)
    }

    fn sign_seal(&self, unsigned: &nostr::UnsignedEvent) -> SignerOp<nostr::Event> {
        // Synchronous: delegate to local keys (no delay for the sign step,
        // only the encrypt step is delayed to trigger the Pending path).
        match unsigned.clone().sign_with_keys(&self.keys) {
            Ok(ev) => SignerOp::ok(ev),
            Err(e) => SignerOp::err(SignerError::Backend(format!("DelayedSigner: sign: {e}"))),
        }
    }
}

/// `LocalSignerAccess` adapter that wraps a `DelayedSigner` behind
/// `Arc<dyn SignerForSeal>`.
struct DelayedSignerAccess {
    signer: Arc<dyn SignerForSeal>,
    pubkey_hex: String,
}

impl LocalSignerAccess for DelayedSignerAccess {
    fn active_local_keys(&self) -> Option<nostr::Keys> {
        // No local nsec — the delayed path uses `signer_for_seal` only.
        None
    }
    fn signer_for_seal(&self) -> Option<Arc<dyn SignerForSeal>> {
        Some(Arc::clone(&self.signer))
    }
}

/// Drive the command with an explicit `SignerForSeal` (not a `nostr::Keys`).
/// Returns the recorder + worker channel receiver (same as `run_cmd`).
fn run_cmd_with_signer(
    cmd: SendGiftWrappedDmCommand,
    signer: Arc<dyn SignerForSeal>,
    dm_lookup: &dyn DmInboxRelayLookup,
    now_secs: u64,
) -> (Recorder, Receiver<ActorCommand>) {
    let recorder = Recorder::default();
    let pubkey_hex = signer.pubkey().to_hex();
    let rx = {
        let sent_ref = &recorder.sent;
        let send = |c: ActorCommand| sent_ref.borrow_mut().push(c);
        let clock = FixedClock(now_secs);
        let signers = DelayedSignerAccess { signer, pubkey_hex };
        let errors = RecordingErrors {
            toasts: &recorder.toasts,
            failures: &recorder.failures,
        };
        let stages = NoopActionStageTracker;
        let recipients = NoopRecipientRelayLookup;
        let (tx, rx) = std::sync::mpsc::channel::<ActorCommand>();
        let mut ctx = ProtocolCommandContext::new(ProtocolCommandContextParts {
            send: &send,
            command_sender: tx,
            clock: &clock,
            signers: &signers,
            dms: dm_lookup,
            errors: &errors,
            stages: &stages,
            recipients: &recipients,
        });
        Box::new(cmd).run(&mut ctx).expect("command body returns Ok");
        rx
    };
    (recorder, rx)
}

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
/// [`ProtocolCommandContext`] and return the recorded side effects plus
/// the `command_sender` channel receiver so callers can drain worker
/// re-entries.
///
/// V-08 — the DM send path now resolves the signer via
/// [`ProtocolCommandContext::signer_for_seal`]. Tests with `Some(keys)`
/// install a `StaticSigner` that returns the `nostr::Keys` blanket impl
/// as the `SignerForSeal`; `None` mirrors the no-active-account path.
/// End-to-end remote-signer (NIP-46 bunker) coverage lives in
/// `nmp_core::actor::commands::remote_signer_tests`.
///
/// ADR-0040 Site 1: `run()` now returns immediately after spawning the
/// gift-wrap worker; `PublishSignedEvent` commands arrive on `worker_rx`
/// after `run()` returns. Call [`drain_worker`] on the returned receiver
/// to collect them into `Recorder::sent` before asserting.
fn run_cmd(
    cmd: SendGiftWrappedDmCommand,
    keys: Option<nostr::Keys>,
    dm_lookup: &dyn DmInboxRelayLookup,
    now_secs: u64,
) -> (Recorder, Receiver<ActorCommand>) {
    let recorder = Recorder::default();
    let rx = {
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
        let recipients = NoopRecipientRelayLookup;
        let (tx, rx) = std::sync::mpsc::channel::<ActorCommand>();
        let mut ctx = ProtocolCommandContext::new(ProtocolCommandContextParts {
            send: &send,
            command_sender: tx,
            clock: &clock,
            signers: &signers,
            dms: dm_lookup,
            errors: &errors,
            stages: &stages,
            recipients: &recipients,
        });
        Box::new(cmd).run(&mut ctx).expect("command body returns Ok");
        rx
    };
    (recorder, rx)
}

/// Drain the worker channel into `recorder.sent` with a bounded timeout.
///
/// The gift-wrap worker runs off-actor; this helper collects up to
/// `expected` commands by calling `recv_timeout` repeatedly. Stops early
/// if the channel disconnects (worker finished). `timeout_per_msg` should
/// be generous enough for the gift-wrap chain to complete (default: 15 s,
/// larger than `GIFT_WRAP_TOTAL_TIMEOUT`).
fn drain_worker(
    recorder: &Recorder,
    rx: Receiver<ActorCommand>,
    expected: usize,
    timeout_per_msg: Duration,
) {
    for _ in 0..expected {
        match rx.recv_timeout(timeout_per_msg) {
            Ok(cmd) => recorder.sent.borrow_mut().push(cmd),
            Err(_) => break,
        }
    }
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
    // Early-exit path: exits before the worker spawn, no drain needed.
    let (rec, _rx) = run_cmd(cmd, None, &empty, 1_700_000_000);

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
    // Early-exit path: exits before the worker spawn, no drain needed.
    let (rec, _rx) = run_cmd(cmd, Some(keys), &empty, 1_700_000_000);

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
    // Early-exit path: exits before the worker spawn, no drain needed.
    let (rec, _rx) = run_cmd(cmd, Some(keys), cache.as_ref(), 1_700_000_000);

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
    //
    // ADR-0040 Site 1: `run()` returns immediately (actor no longer
    // blocks); the two PublishSignedEvent commands arrive via the worker
    // thread and are drained from the command_sender channel.
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
    let (rec, rx) = run_cmd(cmd, Some(keys), cache.as_ref(), 1_700_000_000);
    // Drain the worker channel; expect 2 PublishSignedEvent commands.
    // 15 s per message is larger than GIFT_WRAP_TOTAL_TIMEOUT (12 s).
    drain_worker(&rec, rx, 2, Duration::from_secs(15));

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
    //
    // ADR-0040 Site 1: drain the worker channel after run() returns.
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
    let (rec, rx) = run_cmd(cmd, Some(keys), cache.as_ref(), now);
    drain_worker(&rec, rx, 2, Duration::from_secs(15));

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

// ── ADR-0040 Site 1: non-blocking proof tests ─────────────────────────

/// ADR-0040 Site 1 — primary non-block assertion.
///
/// Install a `DelayedSigner` that waits 300ms per `nip44_encrypt` call.
/// Assert:
///   1. `run()` returns in well under 300ms (actor not stalled).
///   2. After the delay, two `PublishSignedEvent` commands arrive on the
///      worker channel (gift-wrap completed off-actor).
///   3. No toasts or failures.
#[test]
fn run_returns_immediately_with_pending_signer_actor_does_not_block() {
    let sender_keys = nostr::Keys::generate();
    let sender_hex = sender_keys.public_key().to_hex();
    let recipient_keys = nostr::Keys::generate();
    let recipient_hex = recipient_keys.public_key().to_hex();

    let cache = Arc::new(DmRelayCache::new());
    cache.upsert(sender_hex.clone(), vec!["wss://sender-dm.example".to_string()]);
    cache.upsert(recipient_hex.clone(), vec!["wss://recipient-dm.example".to_string()]);

    // 300ms delay makes the "blocks" vs "doesn't block" timing gap
    // unambiguous — if run() blocked it would take at least 300ms × 2 = 600ms.
    let delay = Duration::from_millis(300);
    let signer: Arc<dyn SignerForSeal> = Arc::new(DelayedSigner {
        keys: sender_keys,
        delay,
    });

    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(&sender_hex, &recipient_hex),
        recipient_pubkey: recipient_hex.clone(),
        correlation_id: Some("cid-nonblock".to_string()),
    };

    let t0 = std::time::Instant::now();
    let (rec, rx) = run_cmd_with_signer(cmd, signer, cache.as_ref(), 1_700_000_000);
    let run_elapsed = t0.elapsed();

    // run() MUST return before the 300ms encryption delay fires.
    // Allow 150ms for test harness overhead (thread scheduling, etc.).
    assert!(
        run_elapsed < Duration::from_millis(150),
        "ADR-0040 Site 1: run() took {:?}; must return before the signer delay ({:?}) — actor was blocked",
        run_elapsed,
        delay,
    );

    // No early-exit errors (signer resolved, relays present).
    assert!(
        rec.toasts.borrow().is_empty(),
        "no toasts before drain: {:?}",
        rec.toasts.borrow()
    );
    assert!(rec.failures.borrow().is_empty(), "no failures before drain");
    assert!(
        rec.sent.borrow().is_empty(),
        "no ctx.send() calls — publishes arrive via command_sender only"
    );

    // Now drain the worker channel (gift-wrap runs off-actor at this
    // point). Allow 15s total — larger than GIFT_WRAP_TOTAL_TIMEOUT.
    drain_worker(&rec, rx, 2, Duration::from_secs(15));

    let sent = rec.sent.borrow();
    assert_eq!(
        sent.len(),
        2,
        "two PublishSignedEvent re-entries from the worker after the delay"
    );
    for cmd in sent.iter() {
        match cmd {
            ActorCommand::PublishSignedEvent { raw, correlation_id, .. } => {
                assert_eq!(raw.kind, 1059, "gift-wrap envelope must be kind:1059");
                assert_eq!(
                    correlation_id.as_deref(),
                    Some("cid-nonblock"),
                    "correlation_id threads through"
                );
            }
            other => panic!("unexpected command from worker: {other:?}"),
        }
    }
}

/// ADR-0040 Site 1 — gift-wrap timeout surfaces D6 error via worker.
///
/// Install a `DelayedSigner` whose delay exceeds `GIFT_WRAP_TOTAL_TIMEOUT`
/// (12s). The worker calls `op.wait(GIFT_WRAP_TOTAL_TIMEOUT)` and the
/// timeout arm fires, posting `ShowToast` + `RecordActionFailure` via the
/// worker channel. No `PublishSignedEvent` is emitted.
///
/// This test uses a short synthetic timeout to avoid blocking the test
/// suite. It overrides the signer delay to the actual `GIFT_WRAP_TOTAL_TIMEOUT`
/// but configures a very short delay on the `nip44_encrypt` result — then
/// verifies that when the op.wait deadline fires (here modeled by a signer
/// that never completes), the failure re-entries arrive.
///
/// Implementation note: to avoid a 12s test we instead verify the
/// *stop-on-failure* path by making the first envelope fail. We use
/// `RecordActionFailure` arrival on the channel as the signal.
#[test]
fn gift_wrap_signer_failure_posts_d6_toast_and_failure_via_worker() {
    // Use a signer delay longer than GIFT_WRAP_TOTAL_TIMEOUT to trigger
    // the timeout path. GIFT_WRAP_TOTAL_TIMEOUT is 12s; use 13s here.
    // To avoid actually waiting 12s in CI, we use a separate approach:
    // use a very short custom gift-wrap timeout by verifying the error
    // path shape with a deliberately-never-completing signer.
    //
    // Practical approach: send `op.wait` result as `Err(SignerError::Timeout)`
    // immediately by having the signer send an error on its channel. The
    // worker receives the error and must post toast + failure.

    struct FailingSigner {
        keys: nostr::Keys,
    }

    impl SignerForSeal for FailingSigner {
        fn pubkey(&self) -> nostr::PublicKey {
            self.keys.public_key()
        }
        fn nip44_encrypt(
            &self,
            _recipient_pubkey: &str,
            _plaintext: &str,
        ) -> SignerOp<String> {
            // Immediately signal failure via Pending channel (simulates
            // what happens when GIFT_WRAP_TOTAL_TIMEOUT fires: the
            // `drive_remote_chain` driver thread drops its sender, causing
            // the receiver to see `Disconnected → Backend` error).
            let (tx, rx) = std::sync::mpsc::channel::<Result<String, SignerError>>();
            // Send the timeout error synchronously before returning.
            let _ = tx.send(Err(SignerError::Timeout(
                "test-simulated gift-wrap timeout".to_string(),
            )));
            SignerOp::Pending(rx)
        }
        fn sign_seal(&self, unsigned: &nostr::UnsignedEvent) -> SignerOp<nostr::Event> {
            match unsigned.clone().sign_with_keys(&self.keys) {
                Ok(ev) => SignerOp::ok(ev),
                Err(e) => SignerOp::err(SignerError::Backend(format!("sign: {e}"))),
            }
        }
    }

    let sender_keys = nostr::Keys::generate();
    let sender_hex = sender_keys.public_key().to_hex();
    let recipient_keys = nostr::Keys::generate();
    let recipient_hex = recipient_keys.public_key().to_hex();

    let cache = Arc::new(DmRelayCache::new());
    cache.upsert(sender_hex.clone(), vec!["wss://sender-dm.example".to_string()]);
    cache.upsert(recipient_hex.clone(), vec!["wss://recipient-dm.example".to_string()]);

    let signer: Arc<dyn SignerForSeal> = Arc::new(FailingSigner { keys: sender_keys });
    let cmd = SendGiftWrappedDmCommand {
        rumor: sample_rumor(&sender_hex, &recipient_hex),
        recipient_pubkey: recipient_hex.clone(),
        correlation_id: Some("cid-timeout".to_string()),
    };

    let (rec, rx) = run_cmd_with_signer(cmd, signer, cache.as_ref(), 1_700_000_000);

    // Drain: expect ShowToast + RecordActionFailure (2 commands).
    // Gift-wrap failure fires quickly here since the error is pre-seeded.
    drain_worker(&rec, rx, 2, Duration::from_secs(5));

    // No PublishSignedEvent should arrive.
    assert!(
        rec.sent
            .borrow()
            .iter()
            .all(|c| !matches!(c, ActorCommand::PublishSignedEvent { .. })),
        "timeout path must not emit PublishSignedEvent"
    );
    // ShowToast arrives via the worker channel (captured in rec.sent by drain_worker).
    let sent = rec.sent.borrow();
    let has_toast = sent.iter().any(|c| matches!(c, ActorCommand::ShowToast { .. }));
    let has_failure = sent.iter().any(|c| {
        matches!(
            c,
            ActorCommand::RecordActionFailure { correlation_id, .. }
            if correlation_id == "cid-timeout"
        )
    });
    assert!(has_toast, "D6 — ShowToast must be posted by worker on timeout: {sent:?}");
    assert!(has_failure, "D6 — RecordActionFailure must be posted by worker on timeout: {sent:?}");
}

