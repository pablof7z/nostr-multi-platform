//! NIP-17 DM inbox round-trip — FFI registration chain proofs.
//!
//! These three tests collectively prove the wiring `nmp_app_chirp_register_dm_inbox`
//! sets up — covering the full `kind:1059 → DmInboxProjection →
//! projections["nmp.nip17.dm_inbox"]` round-trip through the public crate
//! surface.
//!
//! Lifted out of `src/ffi.rs` to keep the FFI module under the AGENTS.md
//! 500-LOC hard cap. Living in `tests/` (the integration-test target) means
//! they run against the public `nmp-app-chirp` surface exactly as a host
//! consumer would — same wire as the existing `tests/end_to_end.rs`.
//!
//! ## ADR-0050 §D6 — gift-UNWRAP through the signer port
//!
//! `DmInboxProjection` no longer holds raw `nostr::Keys`; it decrypts each
//! gift-wrap by issuing `ActorCommand::Nip44DecryptForAccount` against the
//! active account (local OR bunker). The first two tests construct an auxiliary
//! projection with a captured command receiver and DRIVE the emitted decrypt
//! commands with the active local key — exactly what the actor's local-backend
//! dispatch arm does inline. The third test signs in for real so the live actor
//! resolves the decrypt port itself.

use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

use nmp_app_chirp::ffi::nmp_app_chirp_register_dm_inbox;
use nmp_core::{ActorMail, CommandSender};
use nmp_core::actor::{ActorCommand};
use nmp_core::actor::{SignCommand};
use nmp_store::{RawEvent, VerifiedEvent};
use nmp_core::substrate::IngestParser;
use nmp_ffi::{
    nmp_app_free, nmp_app_inject_signed_event_json, nmp_app_new, nmp_app_signin_nsec, nmp_app_start,
    NmpApp,
};
use nmp_nip17::{DmInboxProjection, DmInboxSnapshot};
use nostr::nips::nip19::ToBech32;
use nostr::{EventBuilder, Keys, Kind, PublicKey, Tag, Timestamp};

/// Build a signed kind:1059 gift-wrap envelope from `sender` to `receiver`
/// carrying a kind:14 chat-message rumor with `content`, via the pure
/// local-keys composition (`nmp_nip59::gift_wrap_local` — ADR-0050 §D5; the
/// `SignerForSeal` execution model is gone).
fn gift_wrapped_dm(
    sender: &Keys,
    receiver: &PublicKey,
    content: &str,
    created_at: u64,
) -> nostr::Event {
    let rumor = EventBuilder::new(Kind::from_u16(14), content)
        .tags(vec![Tag::public_key(*receiver)])
        .custom_created_at(Timestamp::from(created_at))
        .build(sender.public_key());
    nmp_nip59::gift_wrap_local(sender, receiver, &rumor, Timestamp::from(created_at))
        .expect("gift wrap succeeds")
}

/// Construct an auxiliary projection whose active account is `pubkey`, returning
/// it and the command receiver the harness drains.
fn aux_projection(pubkey: &PublicKey) -> (DmInboxProjection, Receiver<ActorMail>) {
    let (tx, rx) = channel::<ActorMail>();
    let active = Arc::new(Mutex::new(Some(pubkey.to_hex())));
    (DmInboxProjection::new(CommandSender::new(tx), active), rx)
}

/// Drain queued `Nip44DecryptForAccount` commands, decrypting locally with
/// `keys` and invoking each continuation (mirrors the actor's local dispatch
/// arm; each continuation enqueues the next chain step).
fn drive_local_decrypts(rx: &Receiver<ActorMail>, keys: &Keys) {
    while let Ok(mail) = rx.try_recv() {
        let ActorMail::Command(ActorCommand::Sign(SignCommand::Nip44DecryptForAccount {
            peer_pubkey,
            ciphertext,
            continuation,
            ..
        })) = mail
        else {
            continue;
        };
        let outcome = PublicKey::from_hex(&peer_pubkey)
            .map_err(|e| e.to_string())
            .and_then(|peer| {
                nostr::nips::nip44::decrypt(keys.secret_key(), &peer, &ciphertext)
                    .map_err(|e| e.to_string())
            });
        continuation.call(outcome);
    }
}

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

/// THE NIP-17 DM INBOX PORT-CHAIN PROOF: register the DM inbox through the FFI
/// symbol (proving it does not panic / take exclusive ownership), then construct
/// an auxiliary `DmInboxProjection` whose active account is Bob, ingest an
/// Alice→Bob gift-wrap, drive the emitted port decrypts with Bob's local key,
/// and assert the snapshot surfaces the message.
///
/// This proves the gift-wrap → port-unwrap → conversation projection pipeline
/// runs end-to-end for a local account (ADR-0050 §D6).
#[test]
fn dm_inbox_decrypts_through_the_signer_port() {
    let app: *mut NmpApp = nmp_app_new();

    let alice = Keys::generate();
    let bob = Keys::generate();

    // Register the DM inbox through the FFI symbol exactly as Swift does at
    // startup. Load-bearing: it must not panic.
    nmp_app_chirp_register_dm_inbox(app);

    // Auxiliary projection with Bob as the active account.
    let (proj, rx) = aux_projection(&bob.public_key());

    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "hello bob", 12345);
    let envelope_json = nostr::JsonUtil::as_json(&envelope);
    proj.parse(&verified(&envelope));
    drive_local_decrypts(&rx, &bob);

    let snapshot_json = proj.snapshot_json();
    let conversations = snapshot_json
        .get("conversations")
        .and_then(|v| v.as_array())
        .expect("snapshot must carry a `conversations` array");
    assert_eq!(
        conversations.len(),
        1,
        "one conversation expected after one ingest, got {snapshot_json}",
    );
    let convo = &conversations[0];
    assert_eq!(
        convo.get("peer_pubkey").and_then(|v| v.as_str()),
        Some(alice.public_key().to_hex().as_str()),
        "conversation peer must be Alice (the sender), got {convo}",
    );
    let messages = convo
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("conversation must carry a `messages` array");
    assert_eq!(messages.len(), 1, "exactly one decrypted message expected");
    assert_eq!(
        messages[0].get("content").and_then(|v| v.as_str()),
        Some("hello bob"),
        "decrypted content must round-trip verbatim",
    );
    assert_eq!(
        messages[0].get("sender_pubkey").and_then(|v| v.as_str()),
        Some(alice.public_key().to_hex().as_str()),
        "message sender must be Alice (from the verified seal, NOT a tag)",
    );

    nmp_app_free(app);
}

/// THE FFI SNAPSHOT-JSON SHAPE CONTRACT: the JSON the projection surfaces under
/// `projections["nmp.nip17.dm_inbox"]` is exactly the shape `DmInboxSnapshot`
/// serdes to. The Swift consumer decodes this off the kernel update channel; a
/// wire-shape drift here breaks every existing DM screen.
#[test]
fn dm_inbox_snapshot_json_round_trips_through_dm_inbox_snapshot() {
    let app: *mut NmpApp = nmp_app_new();
    let alice = Keys::generate();
    let bob = Keys::generate();

    nmp_app_chirp_register_dm_inbox(app);

    let (proj, rx) = aux_projection(&bob.public_key());
    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "wire-shape check", 700);
    proj.parse(&verified(&envelope));
    drive_local_decrypts(&rx, &bob);

    let snapshot_value = proj.snapshot_json();
    let typed: DmInboxSnapshot = serde_json::from_value(snapshot_value.clone())
        .expect("snapshot JSON must decode to DmInboxSnapshot — wire shape contract");
    assert_eq!(typed.conversations.len(), 1);
    assert_eq!(
        typed.conversations[0].peer_pubkey,
        alice.public_key().to_hex()
    );
    assert_eq!(typed.conversations[0].messages.len(), 1);
    assert_eq!(
        typed.conversations[0].messages[0].content,
        "wire-shape check"
    );

    // Empty-inbox shape contract: with no active account the snapshot surfaces
    // `{"conversations":[], "decrypt_state":"unavailable","undecrypted_count":0}`.
    // ADR-0050 §D7: `remote_signer_unsupported: bool` was replaced by the
    // `decrypt_state` / `undecrypted_count` errors-as-state pair. "unavailable"
    // means no active account; the host hides the DM screen on this token.
    let (tx_empty, _rx_empty) = channel::<ActorMail>();
    let empty_projection =
        DmInboxProjection::new(CommandSender::new(tx_empty), Arc::new(Mutex::new(None)));
    let empty_json = empty_projection.snapshot_json();
    assert_eq!(
        empty_json,
        serde_json::json!({
            "conversations": [],
            "decrypt_state": "unavailable",
            "undecrypted_count": 0
        }),
        "empty-inbox (no active account) must surface decrypt_state:unavailable + undecrypted_count:0",
    );

    nmp_app_free(app);
}

/// THE END-TO-END ROUND-TRIP THROUGH THE FFI REGISTRATION CHAIN + LIVE ACTOR.
///
/// Proves the full kind:1059 gift-wrap → kernel ingest → `IngestParser` →
/// `DmInboxProjection` → `Nip44DecryptForAccount` port (resolved INLINE by the
/// live actor's local-backend dispatch arm) → `projections["nmp.nip17.dm_inbox"]`
/// pipeline is wired correctly, exercised entirely through the public FFI.
///
/// ADR-0050 §D6: Bob signs in for REAL (`nmp_app_signin_nsec` + `nmp_app_start`)
/// so the actor's identity runtime holds his local account and can resolve the
/// inbox's decrypt-port commands itself — the projection never sees `Keys`.
#[test]
fn dm_inbox_full_round_trip_through_ffi() {
    use std::ffi::CString;
    use std::time::Duration;

    let app: *mut NmpApp = nmp_app_new();
    let alice = Keys::generate();
    let bob = Keys::generate();

    // Register the DM inbox IngestParser + the "nmp.nip17.dm_inbox" snapshot
    // projection through the production FFI symbol — exactly the call Swift makes
    // at startup.
    nmp_app_chirp_register_dm_inbox(app);

    // Sign Bob in for real so the actor's identity runtime owns his local
    // account (the decrypt port resolves the active account from the runtime,
    // and the active-pubkey slot is populated for every backend). `Start` is
    // required so the identity reducer runs.
    let bob_nsec = CString::new(bob.secret_key().to_bech32().expect("nsec")).unwrap();
    nmp_app_signin_nsec(app, bob_nsec.as_ptr(), 1);
    nmp_app_start(app, 256, 4);
    std::thread::sleep(Duration::from_millis(200));

    // Build the gift-wrap envelope (kind:1059) from Alice to Bob.
    let envelope = gift_wrapped_dm(&alice, &bob.public_key(), "round-trip", 100);
    let envelope_json = nostr::JsonUtil::as_json(&envelope);

    // Inject the verbatim signed event. The actor ingests it, the DmInboxProjection
    // IngestParser fires, and the two `Nip44DecryptForAccount` commands resolve
    // INLINE on the actor thread (Bob is local) — landing the message.
    let json_cstr = CString::new(envelope_json.as_str()).expect("envelope JSON must be NUL-free");
    let ok = nmp_app_inject_signed_event_json(app, json_cstr.as_ptr());
    assert!(
        ok,
        "nmp_app_inject_signed_event_json must return true for a valid gift-wrap envelope",
    );

    // Give the actor thread time to drain the ingest + decrypt port chain.
    std::thread::sleep(Duration::from_millis(500));

    // Decode via the typed FlatBuffers sidecar (generic JSON lane deleted — rule A6).
    let app_ref: &NmpApp = unsafe { &*app };
    let typed = app_ref.run_typed_snapshot_projections();
    let entry = typed
        .iter()
        .find(|p| p.key == "nmp.nip17.dm_inbox" && !p.payload.is_empty())
        .expect("nmp.nip17.dm_inbox typed projection must be present after nmp_app_chirp_register_dm_inbox");

    let snapshot = nmp_nip17::wire::dm_inbox_fb::decode_dm_inbox_snapshot(&entry.payload)
        .expect("nmp.nip17.dm_inbox payload must decode to DmInboxSnapshot");
    assert_eq!(
        snapshot.conversations.len(),
        1,
        "exactly one conversation expected after one ingest",
    );
    let convo = &snapshot.conversations[0];
    assert_eq!(
        convo.peer_pubkey,
        alice.public_key().to_hex(),
        "conversation peer must be Alice (the sender, taken from the verified seal)",
    );
    assert_eq!(
        convo.messages.len(),
        1,
        "exactly one decrypted message expected"
    );
    assert_eq!(
        convo.messages[0].content, "round-trip",
        "decrypted content must round-trip verbatim from gift_wrap → DmInboxProjection",
    );

    nmp_app_free(app);
}
