//! Stage 3 of NIP-46 wiring: actor-side `RemoteSignerHandle` plumbing.
//!
//! These tests drive the new command handlers + dispatch arms with a stub
//! `RemoteSignerHandle` impl — Stage 4 (broker) ships real NIP-46 transport,
//! but the actor MUST treat the trait as a first-class signer regardless of
//! the impl behind it. D0 stays clean: the stub lives in `nmp-core`'s test
//! tree, NOT in `nmp-signers`.

use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use nmp_signer_iface::SignerOp;
use nostr::nips::nip19::FromBech32;
use nostr::{EventBuilder, Keys, SecretKey, Timestamp};

use super::*;
use crate::actor::commands::identity::{sign_active, IdentityRuntime};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::remote_signer::RemoteSignerHandle;
use crate::substrate::{SignedEvent, UnsignedEvent};

/// nsec from `commands::tests` — known-good test key.
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

/// Stub `RemoteSignerHandle` for Stage 3 plumbing tests. Holds a `Keys` and
/// signs synchronously via `SignerOp::ok(...)`. Production NIP-46 signers
/// live in `nmp-signers`; D0 still forbids that import here so we cannot
/// reach for the real impl — a stub is the correct shape for actor-side
/// plumbing tests.
#[derive(Debug)]
struct StubRemoteSigner {
    keys: Keys,
    pk: String,
    sign_count: Arc<AtomicU32>,
}

impl StubRemoteSigner {
    fn new(keys: Keys) -> Self {
        let pk = keys.public_key().to_hex();
        Self {
            keys,
            pk,
            sign_count: Arc::new(AtomicU32::new(0)),
        }
    }

    fn sign_count_handle(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.sign_count)
    }
}

impl RemoteSignerHandle for StubRemoteSigner {
    fn pubkey_hex(&self) -> String {
        self.pk.clone()
    }

    fn signer_kind(&self) -> &'static str {
        "nip46"
    }

    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        self.sign_count.fetch_add(1, Ordering::Relaxed);
        let kind = nostr::Kind::from_u16(unsigned.kind as u16);
        let tags = unsigned
            .tags
            .iter()
            .filter_map(|t| nostr::Tag::parse(t).ok())
            .collect::<Vec<_>>();
        let built = EventBuilder::new(kind, &unsigned.content)
            .tags(tags)
            .custom_created_at(Timestamp::from(unsigned.created_at))
            .sign_with_keys(&self.keys);
        match built {
            Ok(event) => SignerOp::ok(SignedEvent {
                id: event.id.to_hex(),
                sig: event.sig.to_string(),
                unsigned: UnsignedEvent {
                    pubkey: event.pubkey.to_hex(),
                    kind: event.kind.as_u16() as u32,
                    tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                    content: event.content.clone(),
                    created_at: event.created_at.as_secs(),
                },
            }),
            Err(e) => SignerOp::err(nmp_signer_iface::SignerError::Backend(format!(
                "stub sign failed: {e}"
            ))),
        }
    }

    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String> {
        // Real NIP-44 v2 against the stub's own keys (ADR-0026). The stub must
        // behave like a production signer for actor-side plumbing tests; an
        // error stub would be a landmine for any future test exercising the
        // seal path. D0 still holds — `nostr::nips::nip44` is a leaf crypto
        // crate, not `nmp-signers`.
        let recipient = match nostr::PublicKey::from_hex(recipient_pubkey) {
            Ok(pk) => pk,
            Err(e) => {
                return SignerOp::err(nmp_signer_iface::SignerError::Backend(format!(
                    "stub: invalid recipient pubkey: {e}"
                )))
            }
        };
        SignerOp::Ready(
            nostr::nips::nip44::encrypt(
                self.keys.secret_key(),
                &recipient,
                plaintext,
                nostr::nips::nip44::Version::V2,
            )
            .map_err(|e| nmp_signer_iface::SignerError::Backend(format!("stub nip44 encrypt: {e}"))),
        )
    }

    fn nip44_decrypt(&self, sender_pubkey: &str, ciphertext: &str) -> SignerOp<String> {
        let sender = match nostr::PublicKey::from_hex(sender_pubkey) {
            Ok(pk) => pk,
            Err(e) => {
                return SignerOp::err(nmp_signer_iface::SignerError::Backend(format!(
                    "stub: invalid sender pubkey: {e}"
                )))
            }
        };
        SignerOp::Ready(
            nostr::nips::nip44::decrypt(self.keys.secret_key(), &sender, ciphertext)
                .map_err(|e| {
                    nmp_signer_iface::SignerError::Backend(format!("stub nip44 decrypt: {e}"))
                }),
        )
    }

    fn deliver_rpc_response(&self, _response_json: &str) {
        // Stub: no-op. NIP-46 inbound routing is the broker's job (Stage 4).
    }
}

fn fresh() -> (IdentityRuntime, Kernel) {
    (
        IdentityRuntime::new(new_bunker_handshake_slot()),
        Kernel::new(DEFAULT_VISIBLE_LIMIT),
    )
}

fn stub_signer() -> (Box<StubRemoteSigner>, Arc<AtomicU32>) {
    let sk = SecretKey::from_bech32(TEST_NSEC).expect("valid nsec");
    let keys = Keys::new(sk);
    let stub = StubRemoteSigner::new(keys);
    let count = stub.sign_count_handle();
    (Box::new(stub), count)
}

// ──────────────────────────────────────────────────────────────────────────
// Command-handler tests (the dispatch arms forward straight into these).
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn add_remote_signer_projects_nip46_account_summary() {
    let (mut id, mut kernel) = fresh();
    let (handle, _count) = stub_signer();
    let expected_pk = handle.pubkey_hex();
    add_remote_signer(&mut id, &mut kernel, handle, false);

    let (accounts, active) = kernel.account_snapshot();
    assert!(
        accounts.iter().any(|a| a.signer_kind == "nip46"),
        "expected a nip46 account row, got {accounts:?}"
    );
    let row = accounts
        .iter()
        .find(|a| a.id == expected_pk)
        .expect("row by pubkey hex");
    assert_eq!(row.signer_kind, "nip46");
    assert_eq!(row.status, "active");
    assert!(row.npub.starts_with("npub1"));
    assert_eq!(active, Some(&expected_pk));
    // aim.md §4.4 / §4.5: pre-classified fields the UI binds directly.
    assert_eq!(row.signer_label, "NIP-46");
    assert!(
        row.signer_is_remote,
        "nip46 row must be flagged as a remote signer"
    );
    assert!(row.is_active, "first remote signer becomes active");
}

#[test]
fn bunker_handshake_progress_writes_then_clears() {
    let (id, mut kernel) = fresh();
    bunker_handshake_progress(
        &id,
        &mut kernel,
        "awaiting_pubkey".to_string(),
        Some("connected, waiting for get_public_key".to_string()),
    );
    // D0: handshake state is an app noun — it is written to the identity
    // runtime's shared slot (read by the `"bunker_handshake"` projection),
    // not a typed kernel field.
    let progress = id.bunker_handshake_for_test().expect("set");
    assert_eq!(progress.stage, "awaiting_pubkey");
    assert!(progress.message.is_some());

    // `"idle"` collapses to `None`.
    bunker_handshake_progress(&id, &mut kernel, "idle".to_string(), None);
    assert!(id.bunker_handshake_for_test().is_none());
}

/// Pins the doctrine §6 anti-pattern #1 fix: `BunkerHandshakeDto` carries
/// pre-computed boolean flags + a pre-formatted English `stage_label` so
/// `AccountsView.swift` can render fields directly instead of switching on
/// the raw `stage` string. One assertion block per stage covers every flag
/// transition the shell branches on (visibility guard, cancel-button gate,
/// terminal-icon swap, retry-button label, English subtitle).
#[test]
fn bunker_handshake_dto_pre_computes_view_flags_and_label() {
    let (id, mut kernel) = fresh();

    // ── `"connecting"` — handshake in flight ──────────────────────────────
    bunker_handshake_progress(
        &id,
        &mut kernel,
        "connecting".to_string(),
        Some("dialing wss://r.example".to_string()),
    );
    let dto = id.bunker_handshake_for_test().expect("connecting set");
    assert!(!dto.is_idle, "connecting is not idle");
    assert!(dto.is_in_flight, "connecting is in flight");
    assert!(!dto.is_failed, "connecting has not failed");
    assert!(!dto.is_terminal_success, "connecting is not terminal");
    assert!(dto.can_cancel, "cancel is available while connecting");
    assert_eq!(dto.stage_label, "Connecting to bunker relays…");

    // ── `"awaiting_pubkey"` — also in flight ──────────────────────────────
    bunker_handshake_progress(&id, &mut kernel, "awaiting_pubkey".to_string(), None);
    let dto = id.bunker_handshake_for_test().expect("awaiting set");
    assert!(!dto.is_idle);
    assert!(dto.is_in_flight, "awaiting_pubkey is in flight");
    assert!(!dto.is_failed);
    assert!(!dto.is_terminal_success);
    assert!(dto.can_cancel, "cancel still available awaiting pubkey");
    assert_eq!(dto.stage_label, "Awaiting bunker approval…");

    // ── `"ready"` — terminal success ──────────────────────────────────────
    bunker_handshake_progress(&id, &mut kernel, "ready".to_string(), None);
    let dto = id.bunker_handshake_for_test().expect("ready set");
    assert!(!dto.is_idle);
    assert!(!dto.is_in_flight, "ready is not in flight");
    assert!(!dto.is_failed);
    assert!(dto.is_terminal_success, "ready is the terminal-success flag");
    assert!(!dto.can_cancel, "no cancel once terminal");
    assert_eq!(dto.stage_label, "Connected");

    // ── `"failed"` — terminal failure ─────────────────────────────────────
    bunker_handshake_progress(
        &id,
        &mut kernel,
        "failed".to_string(),
        Some("relay handshake failed".to_string()),
    );
    let dto = id.bunker_handshake_for_test().expect("failed set");
    assert!(!dto.is_idle);
    assert!(!dto.is_in_flight, "failed is not in flight");
    assert!(dto.is_failed, "failed flag tracks terminal failure");
    assert!(!dto.is_terminal_success);
    assert!(!dto.can_cancel, "no cancel once terminal");
    assert_eq!(dto.stage_label, "Bunker handshake failed");
}

#[test]
fn sign_active_routes_through_remote_signer_when_active() {
    let (mut id, mut kernel) = fresh();
    let (handle, count) = stub_signer();
    let expected_pk = handle.pubkey_hex();
    add_remote_signer(&mut id, &mut kernel, handle, false);
    assert_eq!(count.load(Ordering::Relaxed), 0);

    // Drive a publish through the actor path: it must call `sign_active`,
    // which the stub records via `sign_count`.
    let unsigned = UnsignedEvent {
        pubkey: "ignored-by-signer".into(),
        kind: 1,
        tags: Vec::new(),
        content: "stage-3 hello".into(),
        created_at: 1_700_000_000,
    };
    let signed = sign_active(&id, &unsigned).expect("stub sign ok");
    assert_eq!(count.load(Ordering::Relaxed), 1);
    assert_eq!(signed.unsigned.pubkey, expected_pk);
    assert_eq!(signed.unsigned.kind, 1);
    assert_eq!(signed.unsigned.content, "stage-3 hello");
}

#[test]
fn publish_unsigned_event_with_active_remote_uses_stub_signer() {
    // End-to-end: AddRemoteSigner → PublishUnsignedEvent goes through the
    // stub. Mirrors `publish_unsigned_event_signs_and_publishes_arbitrary_kind`
    // from `commands::tests` but with a remote handle behind the active slot.
    //
    // T-publish-resolver-indexer (codex f81f735): seed kind:10002 for the
    // remote signer's pubkey so the resolver has NIP-65 write relays.
    let (mut id, mut kernel) = fresh();
    let (handle, count) = stub_signer();
    let expected_pk = handle.pubkey_hex();
    add_remote_signer(&mut id, &mut kernel, handle, false);
    // Seed kind:10002 so the fail-closed resolver finds write relays.
    kernel.seed_kind10002_for_test(
        &expected_pk,
        &["wss://remote-write-r1.test", "wss://remote-write-r2.test"],
    );

    let unsigned = UnsignedEvent {
        pubkey: "ignored-by-signer".into(),
        kind: 30023,
        tags: vec![vec!["d".into(), "stage-3-article".into()]],
        content: "# hello bunker".into(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event(&id, &mut kernel, unsigned, &mut Vec::new());
    assert_eq!(
        count.load(Ordering::Relaxed),
        1,
        "remote signer was invoked"
    );
    assert!(!outbound.is_empty(), "publish produced outbound frames");
    assert!(outbound[0].text.contains("\"kind\":30023"));
    assert!(outbound[0]
        .text
        .contains(&format!("\"pubkey\":\"{expected_pk}\"")));
    let q = kernel.publish_queue_snapshot();
    assert_eq!(q.last().unwrap().status, "accepted_locally");
}

#[test]
fn send_gift_wrapped_dm_routes_through_remote_signer_adapter() {
    // ADR-0026 Phase 2 end-to-end: with an active bunker (StubRemoteSigner),
    // `send_gift_wrapped_dm` must successfully gift-wrap the rumor TWICE
    // (recipient + self-copy) by routing the seal step through
    // `RemoteSignerForSeal`. Pre-Phase-2 behaviour was a toast naming
    // "ADR-0026 Phase 2"; this test pins the regression.
    let (mut id, mut kernel) = fresh();
    let (handle, sign_count) = stub_signer();
    let sender_pk = handle.pubkey_hex();
    add_remote_signer(&mut id, &mut kernel, handle, false);

    // Recipient must be a real secp256k1 point because NIP-44 ECDH happens
    // against it; a hand-typed hex string would fail at PublicKey::parse.
    let recipient_pk = Keys::generate().public_key().to_hex();
    // Seed kind:10050 for BOTH the recipient AND the sender so the explicit
    // routing path resolves on both envelopes — without these the handler
    // takes the Content-relays fallback, which is correct behaviour but
    // muddies the assertion about which seam was exercised.
    kernel.seed_kind10050_for_test(&recipient_pk, &["wss://recipient-dm.test"]);
    kernel.seed_kind10050_for_test(&sender_pk, &["wss://sender-dm.test"]);

    let rumor = crate::substrate::UnsignedEvent {
        pubkey: sender_pk.clone(),
        kind: 14,
        tags: vec![vec!["p".to_string(), recipient_pk.clone()]],
        content: "hello from a bunker".into(),
        created_at: 0,
    };
    let outbound = super::dm::send_gift_wrapped_dm(&id, &mut kernel, rumor, &recipient_pk, None);

    assert!(
        kernel.last_error_toast_snapshot().is_none(),
        "bunker DM must NOT toast — Phase 2 closes the seam; got toast: {:?}",
        kernel.last_error_toast_snapshot()
    );
    assert!(
        !outbound.is_empty(),
        "both gift-wrap envelopes should produce outbound frames"
    );
    // The stub signed the kind:13 seal TWICE (once per envelope). If the
    // sign count is 0 we would have silently fallen back to a local-key
    // path that does not exist for this account — the seam is the only
    // way to produce a signed envelope here.
    assert_eq!(
        sign_count.load(Ordering::Relaxed),
        2,
        "the remote signer should have signed BOTH seals (recipient + self)"
    );
}

#[test]
fn snapshot_carries_bunker_handshake_value() {
    // D0: NIP-46 bunker handshake is an app noun surfaced via the built-in
    // `"bunker_handshake"` snapshot projection (registered in `nmp_app_new`),
    // NOT a typed `KernelSnapshot` field. This test reproduces that wiring at
    // the kernel level: a projection closure reads the identity runtime's
    // shared slot and the kernel collects it into `projections` on emit.
    let bunker_slot = new_bunker_handshake_slot();
    let id = IdentityRuntime::new(Arc::clone(&bunker_slot));
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Register the `"bunker_handshake"` projection exactly as `nmp_app_new`
    // does — a closure reading the shared slot — and bind it onto the kernel.
    let projections = crate::kernel::new_snapshot_projection_slot();
    {
        let projection_slot = Arc::clone(&bunker_slot);
        projections
            .lock()
            .expect("registry lock")
            .register("bunker_handshake", move || {
                let slot = projection_slot
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                slot.as_ref()
                    .map(|dto| {
                        serde_json::to_value(dto).unwrap_or(serde_json::Value::Null)
                    })
                    .unwrap_or(serde_json::Value::Null)
            });
    }
    kernel.set_snapshot_projection_handle(projections);

    bunker_handshake_progress(
        &id,
        &mut kernel,
        "connecting".to_string(),
        Some("dialing wss://r.example".to_string()),
    );
    let json = kernel.make_update(true);
    assert!(
        json.contains("\"bunker_handshake\""),
        "snapshot must carry the bunker_handshake projection key: {json}"
    );
    assert!(json.contains("\"stage\":\"connecting\""));
}

// ──────────────────────────────────────────────────────────────────────────
// End-to-end dispatch test — drives the new `ActorCommand` variants through
// the spawned `run_actor` loop so the dispatch arms are exercised (not just
// the command-handler functions they wrap).
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn snapshot_carries_nip46_onboarding_projection() {
    // The built-in `"nip46_onboarding"` projection is wired alongside
    // `"bunker_handshake"` and produces a typed DTO with the static
    // signer-app table + pre-computed flags. This end-to-end test drives
    // a `BunkerHandshakeProgress` through the actor and asserts both
    // projections appear in the emitted snapshot.
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::actor::{run_actor_with_observers, ActorCommand};
    use crate::capability_socket::new_capability_callback_slot;
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;

    let (cmd_tx, cmd_rx) = mpsc::channel::<ActorCommand>();
    let (upd_tx, upd_rx) = mpsc::channel::<String>();

    let snapshot_projections = crate::kernel::new_snapshot_projection_slot();
    let bunker_slot = crate::actor::new_bunker_handshake_slot();
    // Replicate the wiring `nmp_app_new` does for the two NIP-46 projections.
    {
        let slot = Arc::clone(&bunker_slot);
        snapshot_projections
            .lock()
            .expect("registry lock")
            .register("bunker_handshake", move || {
                let s = slot.lock().unwrap_or_else(|e| e.into_inner());
                s.as_ref()
                    .map(|dto| serde_json::to_value(dto).unwrap_or(serde_json::Value::Null))
                    .unwrap_or(serde_json::Value::Null)
            });
    }
    {
        let slot = Arc::clone(&bunker_slot);
        snapshot_projections
            .lock()
            .expect("registry lock")
            .register("nip46_onboarding", move || {
                let dto = crate::actor::build_nip46_onboarding_dto(&slot);
                serde_json::to_value(&dto).unwrap_or(serde_json::Value::Null)
            });
    }

    let actor_self_tx = cmd_tx.clone();
    thread::spawn(move || {
        run_actor_with_observers(
            cmd_rx,
            actor_self_tx,
            upd_tx,
            crate::actor::new_lifecycle_observer_slot(),
            crate::actor::new_event_observer_slot(),
            crate::actor::new_raw_event_observer_slot(),
            snapshot_projections,
            #[cfg(feature = "wallet")]
            crate::actor::new_wallet_status_slot(),
            bunker_slot,
            // PR-I: typed slot constructor (was `Arc::new(Mutex::new(Vec::new()))`).
            crate::kernel::new_relay_edit_rows_slot(),
            Arc::new(std::sync::Mutex::new(None)),
            Arc::new(std::sync::Mutex::new(None)),
            new_capability_callback_slot(),
            Arc::new(std::sync::Mutex::new(None)),
            Arc::new(AtomicU64::new(0)),
        );
    });

    cmd_tx
        .send(ActorCommand::Start {
            visible_limit: 50,
            emit_hz: 30,
        })
        .unwrap();

    cmd_tx
        .send(ActorCommand::BunkerHandshakeProgress {
            stage: "connecting".to_string(),
            message: Some("dialing relay".to_string()),
        })
        .unwrap();

    thread::sleep(Duration::from_millis(300));
    let _ = cmd_tx.send(ActorCommand::Shutdown);

    let mut last_frame = String::new();
    while let Ok(frame) = upd_rx.try_recv() {
        last_frame = frame;
    }
    assert!(!last_frame.is_empty(), "actor produced no snapshot frames");
    // Both NIP-46 projection keys must appear and the typed projection's
    // `stage_kind` + `is_in_flight` must reflect the same broker progress.
    assert!(
        last_frame.contains("\"nip46_onboarding\""),
        "snapshot missing nip46_onboarding projection: {last_frame}"
    );
    assert!(
        last_frame.contains("\"stage_kind\":\"connecting\""),
        "nip46_onboarding must carry typed stage_kind: {last_frame}"
    );
    assert!(
        last_frame.contains("\"is_in_flight\":true"),
        "nip46_onboarding must pre-compute is_in_flight=true for connecting: {last_frame}"
    );
    assert!(
        last_frame.contains("\"signer_apps\""),
        "nip46_onboarding must carry signer_apps table: {last_frame}"
    );
}

#[test]
fn dispatch_add_remote_signer_then_progress_surfaces_on_snapshot() {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::actor::{run_actor, ActorCommand};

    let (cmd_tx, cmd_rx) = mpsc::channel::<ActorCommand>();
    let (upd_tx, upd_rx) = mpsc::channel::<String>();
    let actor_self_tx = cmd_tx.clone();
    thread::spawn(move || run_actor(cmd_rx, actor_self_tx, upd_tx));

    cmd_tx
        .send(ActorCommand::Start {
            visible_limit: 50,
            emit_hz: 30,
        })
        .unwrap();

    let (handle, _count) = stub_signer();
    let pk = handle.pubkey_hex();
    cmd_tx
        .send(ActorCommand::AddRemoteSigner { handle })
        .unwrap();
    cmd_tx
        .send(ActorCommand::BunkerHandshakeProgress {
            stage: "ready".to_string(),
            message: None,
        })
        .unwrap();

    // Let the actor drain both commands and emit at least one snapshot.
    thread::sleep(Duration::from_millis(300));
    let _ = cmd_tx.send(ActorCommand::Shutdown);

    let mut last_frame = String::new();
    while let Ok(frame) = upd_rx.try_recv() {
        last_frame = frame;
    }
    assert!(!last_frame.is_empty(), "actor produced no snapshot frames");
    assert!(
        last_frame.contains(&pk),
        "snapshot missing remote-signer pubkey: {last_frame}"
    );
    assert!(
        last_frame.contains("\"signer_kind\":\"nip46\""),
        "snapshot missing nip46 signer_kind: {last_frame}"
    );
    assert!(
        last_frame.contains("\"stage\":\"ready\""),
        "snapshot missing handshake stage=ready: {last_frame}"
    );
}

// ──────────────────────────────────────────────────────────────────────────
// RemoteSignerHandle NIP-44 seam (ADR-0026): the actor reaches NIP-44
// through the same trait it uses for `sign()`. These tests pin the new
// methods on the trait object via the `StubRemoteSigner` double.
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn remote_handle_nip44_round_trips_through_the_seam() {
    // ADR-0026: encrypt to a recipient, then decrypt from that recipient's
    // perspective — NIP-44 is symmetric in the shared conversation key, so a
    // ciphertext sealed by A to B decrypts with B's key against A's pubkey.
    let alice_sk = SecretKey::from_bech32(TEST_NSEC).expect("valid nsec");
    let alice = StubRemoteSigner::new(Keys::new(alice_sk));
    let bob = StubRemoteSigner::new(Keys::generate());

    let alice_pk = RemoteSignerHandle::pubkey_hex(&alice);
    let bob_pk = RemoteSignerHandle::pubkey_hex(&bob);

    let plaintext = "the kind:13 rumor body";
    let ciphertext = RemoteSignerHandle::nip44_encrypt(&alice, &bob_pk, plaintext)
        .wait(std::time::Duration::from_secs(1))
        .expect("encrypt resolves");
    assert_ne!(ciphertext, plaintext, "ciphertext must not be the plaintext");

    let decrypted = RemoteSignerHandle::nip44_decrypt(&bob, &alice_pk, &ciphertext)
        .wait(std::time::Duration::from_secs(1))
        .expect("decrypt resolves");
    assert_eq!(decrypted, plaintext, "round-trip must recover the plaintext");
}

#[test]
fn remote_handle_nip44_encrypt_with_malformed_pubkey_surfaces_err() {
    // D6: a bad hex pubkey through the actor-facing seam must surface as an
    // error, never a panic.
    let (signer, _count) = stub_signer();
    let err = RemoteSignerHandle::nip44_encrypt(&*signer, "not-hex", "plaintext")
        .wait(std::time::Duration::from_millis(100))
        .expect_err("malformed pubkey must surface as Err");
    match err {
        nmp_signer_iface::SignerError::Backend(m) => {
            assert!(m.contains("invalid recipient pubkey"), "got: {m}")
        }
        other => panic!("expected Backend Err, got {other:?}"),
    }
}
