//! ADR-0050 §D6 — gift-UNWRAP through the signer port.
//!
//! These tests drive `DmInboxProjection`'s port-chain end-to-end without a real
//! actor: the projection emits `ActorCommand::Nip44DecryptForAccount` into a
//! `CommandSender` whose receiver the test holds; [`drive_decrypts`] drains each
//! command, performs the NIP-44 decrypt with a supplied [`Decryptor`] (a LOCAL
//! `nostr::Keys` for the local-backend oracle, or a STUB remote signer holding
//! the key out-of-process for the bunker oracle), and invokes the carried
//! continuation — exactly what the actor's `dispatch_cipher_op` arm does
//! (local: inline `Ready`; remote: parked then drained). The projection never
//! holds raw `Keys`; whether decryption happens locally or "out of process" is
//! invisible to it (V-78).

use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::IngestParser;
use nmp_core::{ActorCommand, ActorMail, CommandSender};
use nmp_core::store::{RawEvent, VerifiedEvent};
use nostr::{EventBuilder, JsonUtil, Keys, Kind, PublicKey, SecretKey, Tag, Timestamp};

use super::{DmInboxProjection, KIND_GIFT_WRAP};

/// A NIP-44 decryptor the harness uses to resolve one `Nip44DecryptForAccount`.
/// Models the actor dispatch arm's two backends:
/// * `Local` — the active account is a local key; the actor decrypts INSIDE the
///   runtime. Here the test holds the same `SecretKey` and decrypts directly.
/// * `RemoteStub` — the active account is a bunker; NO local key is present in
///   the projection. The test holds the secret OUT-OF-PROCESS and decrypts on
///   the bunker's behalf — proving the inbox is *structurally* decrypt-capable
///   for a remote signer (the §D6 / Stage-4 oracle).
enum Decryptor {
    Local(SecretKey),
    RemoteStub(SecretKey),
}

impl Decryptor {
    fn secret(&self) -> &SecretKey {
        match self {
            Decryptor::Local(sk) | Decryptor::RemoteStub(sk) => sk,
        }
    }

    fn decrypt(&self, peer_hex: &str, ciphertext: &str) -> Result<String, String> {
        let peer = PublicKey::from_hex(peer_hex).map_err(|e| e.to_string())?;
        nostr::nips::nip44::decrypt(self.secret(), &peer, ciphertext).map_err(|e| e.to_string())
    }
}

/// Drain every queued `Nip44DecryptForAccount` command, resolve it with
/// `decryptor`, and invoke its continuation — until the channel is empty. Each
/// continuation may enqueue the next chain step, so this loop naturally walks
/// the outer→seal→store chain to completion. Returns the number of decrypt
/// commands processed (2 for one fully-unwrapped envelope).
fn drive_decrypts(rx: &Receiver<ActorMail>, decryptor: &Decryptor) -> usize {
    let mut processed = 0;
    while let Ok(mail) = rx.try_recv() {
        let ActorMail::Command(cmd) = mail else {
            continue; // ignore any non-command mail (no relay mail in tests).
        };
        match cmd {
            ActorCommand::Nip44DecryptForAccount {
                peer_pubkey,
                ciphertext,
                continuation,
                ..
            } => {
                processed += 1;
                continuation.call(decryptor.decrypt(&peer_pubkey, &ciphertext));
            }
            // The chain only ever emits decrypts; anything else is unexpected.
            other => panic!("unexpected command on the inbox port chain: {other:?}"),
        }
    }
    processed
}

/// Build a projection whose active account is `pubkey`, returning it (behind an
/// `Arc` so it can be a trait object) and the receiver the harness drains.
fn projection_for(pubkey: &PublicKey) -> (Arc<DmInboxProjection>, Receiver<ActorMail>) {
    let (tx, rx) = channel::<ActorMail>();
    let active = Arc::new(Mutex::new(Some(pubkey.to_hex())));
    let proj = Arc::new(DmInboxProjection::new(CommandSender::new(tx), active));
    (proj, rx)
}

/// Build a signed kind:1059 gift-wrap carrying a kind:14 rumor from `sender` to
/// `receiver` (mirrors NIP-59 §2), via the pure local-keys composition.
fn gift_wrapped_dm(
    sender: &Keys,
    receiver: &PublicKey,
    content: &str,
    created_at: u64,
    reply_to: Option<&str>,
) -> nostr::Event {
    let mut tags = vec![Tag::public_key(*receiver)];
    if let Some(parent) = reply_to {
        tags.push(
            Tag::parse([
                "e".to_string(),
                parent.to_string(),
                String::new(),
                "reply".to_string(),
            ])
            .expect("well-formed e tag"),
        );
    }
    let rumor = EventBuilder::new(Kind::from_u16(14), content)
        .tags(tags)
        .custom_created_at(Timestamp::from(created_at))
        .build(sender.public_key());
    nmp_nip59::gift_wrap_local(sender, receiver, &rumor, Timestamp::from(created_at))
        .expect("gift wrap succeeds")
}

// ── Oracle: existing DM receive tests green for the LOCAL backend ───────────

#[test]
fn local_backend_received_dm_surfaces_through_the_port() {
    // Alice → Bob. Bob's active account is local; the two-step port chain
    // decrypts inline and files the message under peer = Alice.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let (proj, rx) = projection_for(&bob.public_key());

    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "hello bob", 12345, None);
    proj.parse(&verified(&envelope));

    let decrypts = drive_decrypts(&rx, &Decryptor::Local(bob.secret_key().clone()));
    assert_eq!(decrypts, 2, "one envelope = outer + seal = two port decrypts");

    let snap = proj.snapshot();
    assert_eq!(snap.conversations.len(), 1);
    let convo = &snap.conversations[0];
    assert_eq!(convo.peer_pubkey, alice.public_key().to_hex());
    assert_eq!(convo.messages.len(), 1);
    let msg = &convo.messages[0];
    assert_eq!(msg.content, "hello bob");
    assert_eq!(msg.sender_pubkey, alice.public_key().to_hex());
    assert_eq!(msg.created_at, 12345, "D7: the rumor's send time verbatim");
    assert!(!msg.is_outgoing, "Alice→Bob is incoming");
    assert_eq!(
        snap.decrypt_state, "ok",
        "a local account that finished decrypting reports ok (§D7)"
    );
    assert_eq!(snap.undecrypted_count, 0);
}

#[test]
fn local_backend_self_copy_files_under_recipient_peer() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let (proj, rx) = projection_for(&bob.public_key());

    // Self-copy: sender == receiver == Bob, p-tag == Alice.
    let rumor = EventBuilder::new(Kind::from_u16(14), "sent to alice")
        .tags(vec![Tag::public_key(alice.public_key())])
        .custom_created_at(Timestamp::from(500))
        .build(bob.public_key());
    let self_copy = nmp_nip59::gift_wrap_local(&bob, &bob.public_key(), &rumor, Timestamp::from(500))
        .expect("gift wrap");
    proj.parse(&verified(&self_copy));

    drive_decrypts(&rx, &Decryptor::Local(bob.secret_key().clone()));

    let snap = proj.snapshot();
    assert_eq!(snap.conversations.len(), 1);
    assert_eq!(
        snap.conversations[0].peer_pubkey,
        alice.public_key().to_hex(),
        "a self-copy files under the recipient, not the local sender"
    );
    assert!(
        snap.conversations[0].messages[0].is_outgoing,
        "a self-copy authenticated by the local key is outgoing"
    );
}

#[test]
fn local_backend_reply_marker_and_dedupe() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let (proj, rx) = projection_for(&bob.public_key());
    let parent = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccdd00";
    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "re", 300, Some(parent));

    // Deliver twice — the inner rumor id is identical, so it must not duplicate.
    proj.parse(&verified(&envelope));
    proj.parse(&verified(&envelope));
    drive_decrypts(&rx, &Decryptor::Local(bob.secret_key().clone()));

    let snap = proj.snapshot();
    assert_eq!(snap.conversations.len(), 1);
    assert_eq!(snap.conversations[0].messages.len(), 1, "re-delivery is idempotent");
    assert_eq!(
        snap.conversations[0].messages[0].reply_to.as_deref(),
        Some(parent),
        "the NIP-10 reply marker surfaces as reply_to"
    );
}

#[test]
fn envelope_for_another_recipient_does_not_surface() {
    // Alice → Carol. Bob's port decrypt fails (not addressed to Bob) → discard.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let carol = Keys::generate();
    let (proj, rx) = projection_for(&bob.public_key());

    let envelope = gift_wrapped_dm(&alice, &carol.public_key(), "secret", 100, None);
    proj.parse(&verified(&envelope));
    drive_decrypts(&rx, &Decryptor::Local(bob.secret_key().clone()));

    assert!(
        proj.snapshot().conversations.is_empty(),
        "an envelope sealed for Carol must not decrypt for Bob"
    );
}

// ── Oracle: decrypt through a STUB REMOTE signer (no local Keys present) ─────

#[test]
fn bunker_backend_decrypts_through_the_port_with_no_local_keys() {
    // Bob's active account is a bunker: the projection holds NO `Keys` — only
    // Bob's hex pubkey in the active-account slot. The harness decrypts on the
    // bunker's behalf (RemoteStub), proving the inbox is STRUCTURALLY able to
    // unseal a gift-wrap for a remote signer (ADR-0050 §D6 — the V-08 fix).
    let alice = Keys::generate();
    let bob = Keys::generate();
    let (proj, rx) = projection_for(&bob.public_key());

    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "hello bunker bob", 777, None);
    proj.parse(&verified(&envelope));

    // RemoteStub: the secret lives "out of process"; the projection never saw it.
    let decrypts = drive_decrypts(&rx, &Decryptor::RemoteStub(bob.secret_key().clone()));
    assert_eq!(decrypts, 2, "bunker unwrap is still outer + seal = two port decrypts");

    let snap = proj.snapshot();
    assert_eq!(
        snap.conversations.len(),
        1,
        "a bunker account decrypts the inbox through the port (V-08 fix)"
    );
    assert_eq!(snap.conversations[0].messages[0].content, "hello bunker bob");
    assert_eq!(
        snap.conversations[0].peer_pubkey,
        alice.public_key().to_hex()
    );
    assert_eq!(
        snap.decrypt_state, "ok",
        "§D6/§D7: a bunker account decrypts via the port; once drained it is ok"
    );
    assert_eq!(snap.undecrypted_count, 0);
}

// ── Oracle: account-switch leak guard (§D6 epoch) ───────────────────────────

#[test]
fn account_switch_mid_flight_discards_the_stale_completion() {
    // An envelope arrives for Bob (chain captures Bob's epoch). Before the chain
    // resolves, the account switches (clear() bumps the epoch). The terminal
    // insert must be discarded — Bob's plaintext must NOT leak into the new
    // account's snapshot.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let (proj, rx) = projection_for(&bob.public_key());

    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "stale secret", 100, None);
    proj.parse(&verified(&envelope));

    // Account switch happens between ingest (chain launched) and decrypt drain.
    proj.clear();

    let decrypts = drive_decrypts(&rx, &Decryptor::Local(bob.secret_key().clone()));
    assert_eq!(decrypts, 2, "the chain still runs both decrypts");
    assert!(
        proj.snapshot().conversations.is_empty(),
        "a completion under a superseded epoch must be discarded (no cross-account leak)"
    );
}

#[test]
fn not_signed_in_launches_no_chain() {
    // No active account → ingest is a pre-launch no-op: NO port command emitted.
    let (tx, rx) = channel::<ActorMail>();
    let active = Arc::new(Mutex::new(None)); // not signed in
    let proj = DmInboxProjection::new(CommandSender::new(tx), active);

    let alice = Keys::generate();
    let bob = Keys::generate();
    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "hi", 100, None);
    assert!(
        !proj.ingest_gift_wrap(&envelope.as_json(), None),
        "no active account → no chain launched"
    );
    assert!(rx.try_recv().is_err(), "no port command must be emitted");
    let snap = proj.snapshot();
    assert!(snap.conversations.is_empty());
    assert_eq!(
        snap.decrypt_state, "unavailable",
        "no active account → host hides the DM screen (§D7)"
    );
}

#[test]
fn malformed_envelope_launches_no_chain() {
    let bob = Keys::generate();
    let (proj, rx) = projection_for(&bob.public_key());
    assert!(!proj.ingest_gift_wrap("not json", None));
    assert!(!proj.ingest_gift_wrap("{}", None));
    assert!(rx.try_recv().is_err(), "a malformed envelope emits no port command");
    assert!(proj.snapshot().conversations.is_empty());
}

// ── §D7 — bounded per-account decrypt queue + decrypt_state policy ───────────

#[test]
fn bunker_backfill_is_bounded_and_surfaces_limited_state() {
    // Simulate a bunker backfill: many envelopes arrive but their decrypts NEVER
    // resolve (we never drain rx — the bunker round-trips are "outstanding").
    // The projection must admit at most MAX_IN_FLIGHT_DECRYPTS chains, count the
    // rest as over-bound (NEVER silently dropping them), and report `limited`
    // with the full undecrypted count (§D7 — errors-as-state).
    use super::store::MAX_IN_FLIGHT_DECRYPTS;

    let alice = Keys::generate();
    let bob = Keys::generate();
    let (proj, _rx) = projection_for(&bob.public_key()); // rx intentionally undrained

    let total = (MAX_IN_FLIGHT_DECRYPTS as usize) + 5;
    let mut admitted = 0;
    for i in 0..total {
        let envelope = gift_wrapped_dm(&alice, &bob.public_key(), &format!("m{i}"), 100 + i as u64, None);
        if proj.ingest_gift_wrap(&envelope.as_json(), None) {
            admitted += 1;
        }
    }

    assert_eq!(
        admitted as u64, MAX_IN_FLIGHT_DECRYPTS,
        "exactly the bound is admitted; the rest are rejected by the §D7 queue"
    );
    let snap = proj.snapshot();
    assert!(
        snap.conversations.is_empty(),
        "nothing decrypted yet (no bunker round-trip resolved)"
    );
    assert_eq!(snap.decrypt_state, "limited", "pending backfill → limited (§D7)");
    assert_eq!(
        u64::from(snap.undecrypted_count),
        total as u64,
        "EVERY envelope is accounted for — admitted-pending + over-bound — never silently dropped"
    );
}

#[test]
fn drained_backfill_returns_to_ok_and_clears_the_count() {
    // The bound is per-account in-flight: once chains terminate (drain), the
    // count falls and the state returns to ok. Here a LOCAL account drains inline
    // so each ingest's chain completes before the next — it never accumulates.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let (proj, rx) = projection_for(&bob.public_key());

    for i in 0..20u64 {
        let envelope = gift_wrapped_dm(&alice, &bob.public_key(), &format!("m{i}"), 100 + i, None);
        proj.parse(&verified(&envelope));
        drive_decrypts(&rx, &Decryptor::Local(bob.secret_key().clone()));
    }

    let snap = proj.snapshot();
    assert_eq!(snap.conversations.len(), 1, "all 20 land under one peer");
    assert_eq!(snap.conversations[0].messages.len(), 20);
    assert_eq!(
        snap.decrypt_state, "ok",
        "a local account drains inline → never bounded, always ok once settled"
    );
    assert_eq!(snap.undecrypted_count, 0);
}

#[test]
fn account_switch_resets_the_backfill_budget() {
    // A stalled bunker backfill fills the bound; switching accounts (clear())
    // must reset in-flight + over-bound so the new account starts fresh at ok.
    let alice = Keys::generate();
    let bob = Keys::generate();
    let (proj, _rx) = projection_for(&bob.public_key());

    for i in 0..20u64 {
        let envelope = gift_wrapped_dm(&alice, &bob.public_key(), &format!("m{i}"), 100 + i, None);
        let _ = proj.ingest_gift_wrap(&envelope.as_json(), None);
    }
    assert_eq!(proj.snapshot().decrypt_state, "limited", "backfill fills the bound");

    proj.clear(); // account switch
    let snap = proj.snapshot();
    assert_eq!(
        snap.decrypt_state, "ok",
        "clear() resets the §D7 budget — the new account starts ok"
    );
    assert_eq!(snap.undecrypted_count, 0);
}

// ── helper: build a VerifiedEvent from a signed nostr::Event ────────────────

fn verified(ev: &nostr::Event) -> VerifiedEvent {
    let raw = RawEvent {
        id: ev.id.to_hex(),
        pubkey: ev.pubkey.to_hex(),
        created_at: ev.created_at.as_u64(),
        kind: ev.kind.as_u16() as u32,
        tags: ev.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
        content: ev.content.clone(),
        sig: ev.sig.to_string(),
    };
    VerifiedEvent::try_from_raw(raw).expect("real signed event must verify")
}
