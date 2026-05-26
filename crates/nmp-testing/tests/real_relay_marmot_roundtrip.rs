//! PR #631 verification — Marmot kind:445 group-message round-trip over a
//! real relay.
//!
//! Two in-process `MarmotService` instances (Alice, Bob) negotiate an MLS
//! group via the shared `marmot_harness::setup_two_member_group` helper —
//! the in-process channel exchanges Bob's key package, Alice's group create
//! event, the gift-wrapped Welcome, and Bob's post-join commit. After
//! `setup_two_member_group` returns, both services are at the same epoch.
//!
//! Then the kind:445 group message takes the wire path:
//!
//! 1. Alice publishes the encrypted kind:445 to `wss://relay.damus.io` from
//!    her own socket.
//! 2. Bob's socket has subscribed BEFORE the publish (with `since` headroom)
//!    and waits for the inbound EVENT frame.
//! 3. Bob calls `bob.process_message(&delivered)` and asserts the
//!    `ApplicationMessage` plaintext matches what Alice sent.
//!
//! The PR-#631 change under test is the offline-first `emit_now` reordering
//! and the gift-wrap visibility tightening. `setup_two_member_group` exercises
//! the post-migration `MarmotService::wrap_welcome` (which now routes through
//! `gift_wrap_with_signer`) — if the migration broke the welcome path,
//! `setup_two_member_group` would fail before the relay step.
//!
//! `#[ignore]` by default — run with:
//! ```bash
//! cargo test -p nmp-testing --features test-support \
//!   --test real_relay_marmot_roundtrip -- --ignored --nocapture
//! ```

#[path = "marmot_harness.rs"]
mod harness;

use std::net::TcpStream;
use std::sync::Once;
use std::time::{Duration, Instant};

use mdk_core::prelude::MessageProcessingResult;
use nostr::util::JsonUtil as _;
use nostr::{EventBuilder, Keys, Kind, Timestamp};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

type RelaySocket = WebSocket<MaybeTlsStream<TcpStream>>;

const DAMUS_RELAY: &str = "wss://relay.damus.io";
const READ_TIMEOUT: Duration = Duration::from_millis(250);
const EOSE_BUDGET: Duration = Duration::from_secs(8);
const ROUND_TRIP_BUDGET: Duration = Duration::from_secs(20);

fn install_rustls_provider() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

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

#[test]
#[ignore = "real-relay (run with --ignored)"]
fn marmot_kind445_roundtrip_over_damus() {
    // ── Identities + in-process MarmotService setup ─────────────────────────
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let alice_dir = harness::TestDir::new();
    let bob_dir = harness::TestDir::new();
    let alice = harness::service_at(&alice_dir.db_path("alice"), alice_keys.clone());
    let bob = harness::service_at(&bob_dir.db_path("bob"), bob_keys.clone());

    println!(
        "[marmot-rrt] alice={} bob={}",
        alice_keys.public_key().to_hex(),
        bob_keys.public_key().to_hex()
    );

    // setup_two_member_group exercises the migrated `wrap_welcome` (now
    // routes through `gift_wrap_with_signer`). If that broke, this panics.
    let group_id = harness::setup_two_member_group(
        &alice,
        &alice_keys,
        &bob,
        &bob_keys,
        "marmot-real-relay",
    );
    println!("[marmot-rrt] group established at the same epoch on both sides");

    // ── Build the kind:445 group message ────────────────────────────────────
    let plaintext = format!(
        "marmot-rrt: hi bob from alice — ts={}",
        Timestamp::now().as_secs()
    );
    let rumor = EventBuilder::new(Kind::TextNote, &plaintext).build(alice_keys.public_key());
    let msg_event = alice
        .create_message(&group_id, rumor)
        .expect("alice create_message");
    assert_eq!(
        msg_event.kind,
        Kind::MlsGroupMessage,
        "create_message returns kind:445"
    );
    let msg_id = msg_event.id.to_hex();
    let msg_author = msg_event.pubkey.to_hex();
    println!(
        "[marmot-rrt] alice produced kind:445 id={} (signed by exporter pubkey {})",
        msg_id,
        &msg_author[..16]
    );

    // ── Open Bob's socket and subscribe BEFORE publish ──────────────────────
    let mut bob_sock = match open(DAMUS_RELAY) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("SKIP: cannot reach {} (bob socket): {}", DAMUS_RELAY, e);
            return;
        }
    };

    // Filter on the exporter-derived author pubkey kind:445 carries (that's
    // the stable identity per group epoch; `#p` tag presence varies). `since`
    // headroom is small here — kind:445 uses `now()` as `created_at`.
    let since = Timestamp::now().as_secs().saturating_sub(300);
    let req_id = format!("marmot-rrt-{}", &msg_id[..8]);
    let req = format!(
        "[\"REQ\",\"{}\",{{\"kinds\":[445],\"authors\":[\"{}\"],\"since\":{}}}]",
        req_id, msg_author, since
    );
    bob_sock.send(Message::Text(req)).expect("bob REQ");

    let eose_deadline = Instant::now() + EOSE_BUDGET;
    let mut got_eose = false;
    while Instant::now() < eose_deadline && !got_eose {
        match bob_sock.read() {
            Ok(Message::Text(text)) => {
                if text.contains("\"EOSE\"") && text.contains(&req_id) {
                    got_eose = true;
                    break;
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => {
                eprintln!("SKIP: bob socket error before EOSE: {}", e);
                return;
            }
        }
    }
    if !got_eose {
        eprintln!("SKIP: no EOSE within {:?} — relay likely overloaded", EOSE_BUDGET);
        return;
    }
    println!("[marmot-rrt] bob sub is active (EOSE received)");

    // ── Publish via Alice's socket ──────────────────────────────────────────
    let mut alice_sock = match open(DAMUS_RELAY) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("SKIP: cannot reach {} (alice socket): {}", DAMUS_RELAY, e);
            return;
        }
    };
    let publish = format!("[\"EVENT\",{}]", msg_event.as_json());
    alice_sock.send(Message::Text(publish)).expect("alice EVENT");

    // Drain Alice's OK.
    let alice_deadline = Instant::now() + Duration::from_secs(8);
    let mut alice_ok = false;
    while Instant::now() < alice_deadline && !alice_ok {
        match alice_sock.read() {
            Ok(Message::Text(text)) => {
                if text.contains("\"OK\"") && text.contains(&msg_id) {
                    if text.contains("true") {
                        alice_ok = true;
                    } else {
                        let _ = alice_sock.close(None);
                        let _ = bob_sock.close(None);
                        panic!("relay rejected the kind:445 publish: {}", text);
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
                eprintln!("[marmot-rrt] alice socket error after publish: {}", e);
                break;
            }
        }
    }
    let _ = alice_sock.close(None);
    if !alice_ok {
        eprintln!(
            "SKIP: no OK from relay for kind:445 publish within 8s — relay likely throttling"
        );
        let _ = bob_sock.close(None);
        return;
    }
    println!("[marmot-rrt] relay ACK'd the publish");

    // ── Wait for Bob's socket to deliver the encrypted event ────────────────
    let deadline = Instant::now() + ROUND_TRIP_BUDGET;
    let mut delivered: Option<nostr::Event> = None;
    while Instant::now() < deadline && delivered.is_none() {
        match bob_sock.read() {
            Ok(Message::Text(text)) => {
                if !text.contains(&req_id) || !text.contains("\"EVENT\"") {
                    continue;
                }
                let val: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let arr = match val.as_array() {
                    Some(a) => a,
                    None => continue,
                };
                if arr.len() < 3 {
                    continue;
                }
                let ev: nostr::Event = match serde_json::from_value(arr[2].clone()) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                if ev.id.to_hex() == msg_id {
                    delivered = Some(ev);
                    break;
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => {
                eprintln!("[marmot-rrt] bob socket error: {}", e);
                break;
            }
        }
    }
    let _ = bob_sock.close(None);

    let delivered = match delivered {
        Some(e) => e,
        None => {
            eprintln!(
                "SKIP: bob never received kind:445 id={} within {:?}",
                msg_id, ROUND_TRIP_BUDGET
            );
            return;
        }
    };

    // ── Decrypt via MarmotService and assert plaintext ──────────────────────
    match bob
        .process_message(&delivered)
        .expect("bob.process_message must succeed for a valid kind:445 in our group")
    {
        MessageProcessingResult::ApplicationMessage(m) => {
            assert_eq!(
                m.content, plaintext,
                "decrypted plaintext must match what alice sent"
            );
            assert_eq!(
                m.pubkey,
                alice_keys.public_key(),
                "the MLS-attested sender must be alice"
            );
            println!(
                "[marmot-rrt] OK: bob decrypted alice's kind:445; plaintext={:?}",
                m.content
            );
        }
        other => panic!(
            "expected ApplicationMessage for the relay-delivered kind:445, got {other:?}"
        ),
    }
}
