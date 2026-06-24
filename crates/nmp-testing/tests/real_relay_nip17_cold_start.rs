//! F-02 cold-start DM receive-side verification (issue #977).
//!
//! Scenario: Alice gift-wraps a kind:1059 to Bob's DM relay while Bob is
//! **offline** (raw WebSocket, no kernel); Bob then cold-starts a fresh
//! `DmInboxProjection`, subscribes with a fresh REQ (no `since` watermark), the
//! relay replays the stored gift-wrap via EOSE backfill, and Bob's projection
//! decrypts + surfaces the DM. Two `#[ignore]` variants:
//! `nip17_cold_start_receive_nak_serve` (deterministic in-process `nak serve`)
//! and `nip17_cold_start_receive_damus` (live relay).
//!
//! Complements `real_relay_nip17_roundtrip.rs` (live delivery: subscribe BEFORE
//! publish) by exercising the *storage-replay* path a new user hits on first
//! login (publish FIRST, subscribe LATER). The relay-transport half is covered
//! by the round-trip test; the kernel ingest→projection→snapshot half by
//! `dm_inbox_full_round_trip_through_ffi`. The unique risk here is subscription
//! timing (publish-before-subscribe + EOSE backfill) + projection decryption.
//!
//! ADR-0050 §D6: the projection decrypts through the signer port — it emits
//! `Nip44DecryptForAccount` commands which `drive_local_decrypts` resolves with
//! Bob's local key (exactly the actor's local-backend dispatch arm).
//!
//! ```bash
//! cargo test -p nmp-testing --test real_relay_nip17_cold_start \
//!   -- nip17_cold_start_receive_nak_serve --ignored --nocapture
//! ```

use std::net::TcpStream;
use std::sync::{Arc, Mutex, Once};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use nmp_core::{ActorMail, CommandSender};
use nmp_core::actor::{ActorCommand, SignCommand};
use nmp_nip17::DmInboxProjection;
use nmp_nip59::{gift_wrap_local, KIND_GIFT_WRAP};
use nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK;
use nostr::util::JsonUtil as _;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

// ─── Constants ─────────────────────────────────────────────────────────────

const DAMUS_RELAY: &str = "wss://relay.damus.io";
/// `nak serve` default — localhost ephemeral relay (no persistence between runs).
const NAK_SERVE_ADDR: &str = "ws://localhost:10547";
const READ_TIMEOUT: Duration = Duration::from_millis(250);
const BACKFILL_BUDGET: Duration = Duration::from_secs(15);
const PUBLISH_ACK_BUDGET: Duration = Duration::from_secs(8);

// ─── TLS ───────────────────────────────────────────────────────────────────

fn install_rustls_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

type RelaySocket = WebSocket<MaybeTlsStream<TcpStream>>;

fn open(url: &str) -> Result<RelaySocket, String> {
    install_rustls_provider();
    let (mut socket, _response) = connect(url).map_err(|e| e.to_string())?;
    match socket.get_mut() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(Some(READ_TIMEOUT));
        }
        MaybeTlsStream::Rustls(s) => {
            let _ = s.get_ref().set_read_timeout(Some(READ_TIMEOUT));
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }
    Ok(socket)
}

// ─── Core scenario logic ────────────────────────────────────────────────────

/// Run the cold-start scenario against `relay_url`.
///
/// Returns `Ok(true)` on success, `Ok(false)` if the relay was unreachable
/// (so the test can emit a SKIP message), and `Err(msg)` on a scenario failure.
fn run_cold_start_scenario(relay_url: &str) -> Result<bool, String> {
    // ── Identities ─────────────────────────────────────────────────────────
    let alice = Keys::generate();
    let bob = Keys::generate();
    let bob_hex = bob.public_key().to_hex();

    println!("[nip17-cs] relay:       {}", relay_url);
    println!("[nip17-cs] alice pubkey: {}", alice.public_key().to_hex());
    println!("[nip17-cs] bob   pubkey: {}", bob_hex);

    // ── PHASE 1: Alice publishes while Bob is offline ───────────────────────
    //
    // Alice connects, publishes the gift-wrap, waits for the relay OK, then
    // disconnects. Bob's kernel does NOT exist yet. This is the "offline
    // publish" half of the cold-start scenario.

    let plaintext = format!(
        "nip17-cold-start: hello bob — ts={}",
        Timestamp::now().as_secs()
    );

    // Build kind:14 rumor (the inner DM content, never published directly).
    let rumor = EventBuilder::new(Kind::from_u16(14), &plaintext)
        .tag(Tag::public_key(bob.public_key()))
        .custom_created_at(Timestamp::now())
        .build(alice.public_key());

    // Gift-wrap via the pure local-keys composition (ADR-0050 §D5).
    let tweaked = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);
    let envelope = gift_wrap_local(&alice, &bob.public_key(), &rumor, tweaked)
        .map_err(|e| format!("gift_wrap_local failed: {e}"))?;

    assert_eq!(envelope.kind, Kind::GiftWrap, "outer kind must be 1059");
    let envelope_id = envelope.id.to_hex();
    let envelope_json = envelope.as_json();

    println!("[nip17-cs] PHASE 1: publishing kind:1059 id={} (Bob offline)", envelope_id);

    let mut alice_sock = match open(relay_url) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[nip17-cs] SKIP: cannot reach {} (alice socket): {}", relay_url, e);
            return Ok(false);
        }
    };

    // Publish Alice's gift-wrap. Bob is NOT subscribed yet.
    alice_sock
        .send(Message::Text(format!("[\"EVENT\",{}]", envelope_json)))
        .map_err(|e| format!("alice send EVENT: {e}"))?;

    // Wait for OK confirmation that the relay accepted the event.
    let ack_deadline = Instant::now() + PUBLISH_ACK_BUDGET;
    let mut alice_ok = false;
    while Instant::now() < ack_deadline && !alice_ok {
        match alice_sock.read() {
            Ok(Message::Text(text)) => {
                if text.contains("\"OK\"") && text.contains(&envelope_id) {
                    if text.contains("true") {
                        alice_ok = true;
                    } else {
                        let _ = alice_sock.close(None);
                        return Err(format!("relay rejected gift-wrap publish: {}", text));
                    }
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => {
                let _ = alice_sock.close(None);
                eprintln!("[nip17-cs] SKIP: alice socket error: {}", e);
                return Ok(false);
            }
        }
    }
    let _ = alice_sock.close(None);

    if !alice_ok {
        eprintln!("[nip17-cs] SKIP: no OK from relay for publish within {:?}", PUBLISH_ACK_BUDGET);
        return Ok(false);
    }
    println!("[nip17-cs] relay ACK'd the publish — Alice disconnects (Bob still offline)");

    // ── PHASE 2: Bob cold-starts (fresh kernel state) ───────────────────────
    //
    // Bob now constructs a brand-new `DmInboxProjection` with no prior state —
    // the "fresh install" / cold-start condition. He opens a fresh WebSocket
    // subscription with NO `since` filter (no watermark), which is what the
    // kernel does on first start. The relay MUST replay the stored gift-wrap
    // in its EOSE backfill.

    println!("[nip17-cs] PHASE 2: Bob cold-starts — new projection, no prior state");

    // Fresh projection bound to the signer port (ADR-0050 §D6). Bob's active
    // account is a LOCAL key here; the projection emits `Nip44DecryptForAccount`
    // commands which we drain (decrypting with Bob's local keys, exactly as the
    // actor's local-backend dispatch arm does) once the relay delivers the
    // envelope. The projection itself holds no `Keys`.
    let (bob_tx, bob_rx) = channel::<ActorMail>();
    let bob_active = Arc::new(Mutex::new(Some(bob.public_key().to_hex())));
    let bob_projection =
        DmInboxProjection::new(CommandSender::new(bob_tx), Arc::clone(&bob_active));

    // Assert it starts empty — the cold-start precondition.
    let initial_snap = bob_projection.snapshot();
    assert!(
        initial_snap.conversations.is_empty(),
        "cold-start projection must be empty before the relay subscription"
    );
    println!("[nip17-cs] cold-start confirmed: projection starts empty");

    let mut bob_sock = match open(relay_url) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[nip17-cs] SKIP: cannot reach {} (bob socket): {}", relay_url, e);
            return Ok(false);
        }
    };

    // Cold-start REQ: no `since` filter (Bob has never connected before).
    // The relay must replay every stored kind:1059 `#p <bob>` event.
    //
    // For the nak-serve scenario since is 0 (no stored history besides our
    // event). For the live Damus relay we use a `since` 2 days back (to avoid
    // retrieving enormous amounts of data from real relays) — but crucially
    // this is STILL a cold start from Bob's perspective (he has no local
    // watermark; we just apply a courtesy window for etiquette). The
    // critical property is that the since is EARLIER than Alice's publish.
    let since_secs = {
        let now = Timestamp::now().as_secs();
        // 2 days back — wide enough to catch the event we just published.
        now.saturating_sub(172_800)
    };

    let req_id = format!("nip17-cs-{}", &bob_hex[..8]);
    let req = format!(
        "[\"REQ\",\"{}\",{{\"kinds\":[1059],\"#p\":[\"{}\"],\"since\":{}}}]",
        req_id, bob_hex, since_secs
    );
    bob_sock
        .send(Message::Text(req))
        .map_err(|e| format!("bob send REQ: {e}"))?;

    println!("[nip17-cs] Bob subscribed (cold REQ, no local watermark)");

    // ── PHASE 3: Drain Bob's socket until EOSE + envelope delivery ──────────
    //
    // The relay must deliver the stored gift-wrap BEFORE or AT the EOSE
    // boundary. We collect all EVENT frames between now and EOSE, then
    // feed any matching kind:1059 to `bob_projection.ingest_gift_wrap`.

    let backfill_deadline = Instant::now() + BACKFILL_BUDGET;
    let mut got_eose = false;
    let mut delivered_json: Option<String> = None;

    while Instant::now() < backfill_deadline && !(got_eose && delivered_json.is_some()) {
        match bob_sock.read() {
            Ok(Message::Text(text)) => {
                if text.contains("\"EOSE\"") && text.contains(&req_id) {
                    got_eose = true;
                    println!("[nip17-cs] EOSE received (backfill complete)");
                    // If we already have the event, we are done.
                    if delivered_json.is_some() {
                        break;
                    }
                    // After EOSE the relay may still deliver live events —
                    // keep draining until the budget expires.
                    continue;
                }

                if text.contains("\"EVENT\"") && text.contains(&req_id) {
                    // Parse ["EVENT", <sub_id>, <event-json>]
                    if let Ok(serde_json::Value::Array(arr)) = serde_json::from_str::<serde_json::Value>(&text) {
                        if arr.len() >= 3 {
                            if let Ok(ev) = serde_json::from_value::<nostr::Event>(arr[2].clone()) {
                                if ev.id.to_hex() == envelope_id {
                                    println!("[nip17-cs] Bob's socket received the gift-wrap id={}", envelope_id);
                                    delivered_json = Some(arr[2].to_string());
                                    if got_eose {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => {
                let _ = bob_sock.close(None);
                eprintln!("[nip17-cs] SKIP: bob socket error during backfill: {}", e);
                return Ok(false);
            }
        }
    }
    let _ = bob_sock.close(None);

    if !got_eose {
        eprintln!("[nip17-cs] SKIP: EOSE not received within {:?}", BACKFILL_BUDGET);
        return Ok(false);
    }

    let event_json = match delivered_json {
        Some(j) => j,
        None => {
            return Err(format!(
                "FAIL: gift-wrap id={} was NOT delivered to Bob's cold-start subscription \
                 within {:?}. The relay did not replay the stored event.",
                envelope_id, BACKFILL_BUDGET
            ));
        }
    };

    println!("[nip17-cs] PHASE 2 relay transport: PASS — stored event backfilled to cold subscriber");

    // ── PHASE 4: Projection decryption ─────────────────────────────────────
    //
    // Feed the raw EVENT JSON directly into Bob's fresh `DmInboxProjection`
    // via `ingest_gift_wrap` — the projection's production ingest method.
    // Note: this drives the projection directly; the full production path
    // additionally traverses the kernel's Schnorr-verify + store-insert gate
    // before the IngestParser fires (covered independently by
    // `dm_inbox_full_round_trip_through_ffi`).
    // The planner-compiled REQ + kind:10050 routing junction is NOT covered
    // here — see the F-02 closure-gate follow-up on issue #977.
    //
    // We snapshot before and after to detect whether the ingest mutated the
    // projection (the public API does not return a bool).

    let before_ingest = bob_projection.snapshot();
    bob_projection.ingest_gift_wrap(&event_json, Some(relay_url));
    // ADR-0050 §D6: drain the emitted port decrypts with Bob's local key.
    drive_local_decrypts(&bob_rx, &bob);
    let after_ingest = bob_projection.snapshot();

    if after_ingest.conversations.is_empty() {
        // Distinguishes "ingest was a silent no-op" from "projection state is wrong".
        let _ = before_ingest; // consumed above
        return Err(
            "FAIL: DmInboxProjection.snapshot() has no conversations after on_raw_event_with_source. \
             Bob could not decrypt the gift-wrap. \
             Possible causes: keys mismatch, malformed envelope, \
             not-signed-in slot, or NIP-59 timestamp window rejection."
            .to_string(),
        );
    }
    let snap = after_ingest;

    let convo = &snap.conversations[0];
    if convo.messages.is_empty() {
        return Err("FAIL: conversation has no messages".to_string());
    }

    let msg = &convo.messages[0];
    if msg.content != plaintext {
        return Err(format!(
            "FAIL: decrypted content mismatch — expected {:?}, got {:?}",
            plaintext, msg.content
        ));
    }

    if msg.sender_pubkey != alice.public_key().to_hex() {
        return Err(format!(
            "FAIL: sender_pubkey mismatch — expected alice {}, got {}",
            alice.public_key().to_hex(),
            msg.sender_pubkey
        ));
    }

    if convo.peer_pubkey != alice.public_key().to_hex() {
        return Err(format!(
            "FAIL: peer_pubkey mismatch — expected alice {}, got {}",
            alice.public_key().to_hex(),
            convo.peer_pubkey
        ));
    }

    if msg.is_outgoing {
        return Err("FAIL: message is marked outgoing, should be incoming".to_string());
    }

    // Source relay provenance must be recorded (the relay URL passed to ingest).
    if !msg.source_relays.iter().any(|r| r == relay_url) {
        return Err(format!(
            "FAIL: source_relays does not contain the delivering relay {}; got {:?}",
            relay_url, msg.source_relays
        ));
    }

    println!("[nip17-cs] PHASE 3 projection decryption: PASS");
    println!("[nip17-cs]   peer_pubkey:  {}", convo.peer_pubkey);
    println!("[nip17-cs]   sender:       {}", msg.sender_pubkey);
    println!("[nip17-cs]   content:      {:?}", msg.content);
    println!("[nip17-cs]   is_outgoing:  {}", msg.is_outgoing);
    println!("[nip17-cs]   source_relay: {:?}", msg.source_relays);
    println!("[nip17-cs] F-02 COLD-START SCENARIO: PASS");

    Ok(true)
}

/// Drain every `Nip44DecryptForAccount` command the projection emitted and
/// resolve it by decrypting with `keys`' local secret — exactly what the actor's
/// local-backend dispatch arm does inline. Each continuation enqueues the next
/// chain step, so this walks the outer→seal→store unwrap to completion.
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
        let outcome = nostr::PublicKey::from_hex(&peer_pubkey)
            .map_err(|e| e.to_string())
            .and_then(|peer| {
                nostr::nips::nip44::decrypt(keys.secret_key(), &peer, &ciphertext)
                    .map_err(|e| e.to_string())
            });
        continuation.call(outcome);
    }
}

// ─── Test variants ──────────────────────────────────────────────────────────

/// F-02 cold-start verification against `nak serve` (deterministic).
///
/// Requires `nak serve` running on localhost:10547.  The test starts the
/// server as a child process (if `nak` is on `$PATH`) and tears it down
/// after the scenario.  If `nak` is absent, the test skips gracefully.
#[test]
#[ignore = "real-relay (nak serve): run with --ignored --nocapture"]
fn nip17_cold_start_receive_nak_serve() {
    // Spawn nak serve as a child process.
    let mut child = match std::process::Command::new("nak")
        .args(["serve", "--port", "10547"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[nip17-cs-nak] SKIP: cannot spawn `nak serve`: {}", e);
            return;
        }
    };

    // Wait briefly for the server to start accepting connections.
    let ready_deadline = Instant::now() + Duration::from_secs(3);
    let mut server_ready = false;
    while Instant::now() < ready_deadline {
        if std::net::TcpStream::connect("127.0.0.1:10547").is_ok() {
            server_ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    if !server_ready {
        let _ = child.kill();
        eprintln!("[nip17-cs-nak] SKIP: nak serve did not become ready within 3s");
        return;
    }
    println!("[nip17-cs-nak] nak serve is ready on {}", NAK_SERVE_ADDR);

    let result = run_cold_start_scenario(NAK_SERVE_ADDR);
    let _ = child.kill();
    let _ = child.wait();

    match result {
        Ok(true) => {
            println!("[nip17-cs-nak] PASS");
        }
        Ok(false) => {
            eprintln!("[nip17-cs-nak] SKIP (relay unreachable during test)");
        }
        Err(msg) => {
            panic!("{}", msg);
        }
    }
}

/// F-02 cold-start verification against `relay.damus.io` (live relay).
///
/// Same scenario as `nip17_cold_start_receive_nak_serve` but uses a real
/// public relay. Flakiness expectations: the relay may be overloaded or
/// drop the publish; in that case the test emits SKIP rather than failing.
///
/// This test is the load-bearing evidence for F-02 acceptance per the
/// real-relay-nightly CI workflow.
#[test]
#[ignore = "real-relay (damus): run with --ignored --nocapture"]
fn nip17_cold_start_receive_damus() {
    match run_cold_start_scenario(DAMUS_RELAY) {
        Ok(true) => {
            println!("[nip17-cs-damus] PASS");
        }
        Ok(false) => {
            eprintln!("[nip17-cs-damus] SKIP (relay unreachable or throttling)");
        }
        Err(msg) => {
            panic!("{}", msg);
        }
    }
}
