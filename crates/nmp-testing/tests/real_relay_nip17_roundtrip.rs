//! PR #631 verification — NIP-17 gift-wrap round-trip over a real relay.
//!
//! Two raw WebSocket sockets to `wss://relay.damus.io`:
//!
//! * Alice's socket — used to PUBLISH a kind:1059 gift-wrap addressed to Bob.
//! * Bob's socket   — used to SUBSCRIBE with `{"kinds":[1059],"#p":[bob]}`
//!   and decrypt the inbound envelope via `nmp_nip59::unwrap_gift_wrap`.
//!
//! Bob's REQ is sent (and EOSE awaited) BEFORE Alice publishes. Storage-and-
//! replay behaviour varies by relay; this ordering guarantees the relay
//! delivers the live frame on Bob's socket regardless.
//!
//! Crucially, the publish path here goes through `nmp_nip59::gift_wrap_with_signer`
//! — the only public surface after the PR-#631 visibility tightening. The
//! decrypt path uses `nmp_nip59::unwrap_gift_wrap`. If the migration to the
//! signer seam broke either side, this test will fail loudly with either a
//! seal-encryption error or a decryption error.
//!
//! `#[ignore]` by default — run with:
//! ```bash
//! cargo test -p nmp-testing --features test-support \
//!   --test real_relay_nip17_roundtrip -- --ignored --nocapture
//! ```

use std::net::TcpStream;
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use nmp_nip59::{gift_wrap_with_signer, unwrap_gift_wrap, SignerForSeal, GIFT_WRAP_TOTAL_TIMEOUT};
use nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK;
use nostr::util::JsonUtil as _;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

type RelaySocket = WebSocket<MaybeTlsStream<TcpStream>>;

const DAMUS_RELAY: &str = "wss://relay.damus.io";
const READ_TIMEOUT: Duration = Duration::from_millis(250);
const EOSE_BUDGET: Duration = Duration::from_secs(8);
const ROUND_TRIP_BUDGET: Duration = Duration::from_secs(20);

/// One-time TLS provider install (mirrors `relay_worker::install_rustls_provider`).
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
fn nip17_giftwrap_roundtrip_over_damus() {
    // ── Identities ──────────────────────────────────────────────────────────
    let alice = Keys::generate();
    let bob = Keys::generate();
    let bob_hex = bob.public_key().to_hex();

    println!("[nip17-rrt] alice pubkey: {}", alice.public_key().to_hex());
    println!("[nip17-rrt] bob   pubkey: {}", bob_hex);

    // ── Open Bob's socket FIRST and subscribe BEFORE Alice publishes ────────
    let mut bob_sock = match open(DAMUS_RELAY) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("SKIP: cannot reach {} (bob socket): {}", DAMUS_RELAY, e);
            return;
        }
    };

    // since: 2 days back. `Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK)`
    // can land up to 172_800 seconds in the past; without this headroom the
    // relay would silently drop our own gift-wrap from the live frame.
    let now_secs = Timestamp::now().as_secs();
    let since = now_secs.saturating_sub(200_000);
    let req_id = format!("nip17-rrt-{}", &bob_hex[..8]);
    let req = format!(
        "[\"REQ\",\"{}\",{{\"kinds\":[1059],\"#p\":[\"{}\"],\"since\":{}}}]",
        req_id, bob_hex, since
    );
    bob_sock.send(Message::Text(req)).expect("bob REQ");

    // Wait for EOSE so we know the sub is active on the relay before publish.
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
        eprintln!(
            "SKIP: no EOSE within {:?} — relay likely overloaded",
            EOSE_BUDGET
        );
        return;
    }
    println!("[nip17-rrt] bob sub is active (EOSE received)");

    // ── Build the kind:14 rumor (NIP-17 chat-message shape) ─────────────────
    let plaintext = format!(
        "nip17-rrt: hello bob from alice — ts={}",
        Timestamp::now().as_secs()
    );
    let rumor = EventBuilder::new(Kind::from_u16(14), &plaintext)
        .tag(Tag::public_key(bob.public_key()))
        .custom_created_at(Timestamp::now())
        .build(alice.public_key());

    // ── Gift-wrap via the PR-public seam ────────────────────────────────────
    // `nmp_nip59::gift_wrap_with_signer` is the ONLY public path post-#631.
    // The blanket `SignerForSeal` impl on `nostr::Keys` resolves every step
    // synchronously, so `.wait` returns immediately.
    let signer: Arc<dyn SignerForSeal> = Arc::new(alice.clone());
    let tweaked = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);
    let envelope = gift_wrap_with_signer(&signer, &bob.public_key(), &rumor, tweaked)
        .wait(GIFT_WRAP_TOTAL_TIMEOUT)
        .expect("gift_wrap_with_signer must succeed for local keys");
    assert_eq!(envelope.kind, Kind::GiftWrap, "outer kind must be 1059");
    let envelope_id = envelope.id.to_hex();
    let envelope_json = envelope.as_json();

    // ── Publish via Alice's socket ──────────────────────────────────────────
    let mut alice_sock = match open(DAMUS_RELAY) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("SKIP: cannot reach {} (alice socket): {}", DAMUS_RELAY, e);
            return;
        }
    };
    let publish = format!("[\"EVENT\",{}]", envelope_json);
    alice_sock
        .send(Message::Text(publish))
        .expect("alice EVENT");
    println!("[nip17-rrt] alice published kind:1059 id={}", envelope_id);

    // Drain Alice's OK so we know the relay accepted the publish before we
    // wait on Bob's side.
    let alice_deadline = Instant::now() + Duration::from_secs(8);
    let mut alice_ok = false;
    while Instant::now() < alice_deadline && !alice_ok {
        match alice_sock.read() {
            Ok(Message::Text(text)) => {
                if text.contains("\"OK\"") && text.contains(&envelope_id) {
                    if text.contains("true") {
                        alice_ok = true;
                    } else {
                        let _ = alice_sock.close(None);
                        let _ = bob_sock.close(None);
                        panic!("relay rejected the gift-wrap publish: {}", text);
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
                eprintln!("[nip17-rrt] alice socket error after publish: {}", e);
                break;
            }
        }
    }
    let _ = alice_sock.close(None);
    if !alice_ok {
        eprintln!("SKIP: no OK from relay for our publish within 8s — relay likely throttling");
        let _ = bob_sock.close(None);
        return;
    }
    println!("[nip17-rrt] relay ACK'd the publish");

    // ── Wait for Bob's socket to deliver the envelope ───────────────────────
    let deadline = Instant::now() + ROUND_TRIP_BUDGET;
    let mut delivered: Option<nostr::Event> = None;
    while Instant::now() < deadline && delivered.is_none() {
        match bob_sock.read() {
            Ok(Message::Text(text)) => {
                // EVENT frames addressed to our sub carry our req_id.
                if !text.contains(&req_id) || !text.contains("\"EVENT\"") {
                    continue;
                }
                // Parse `["EVENT", <sub_id>, <event-json>]`.
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
                if ev.id.to_hex() == envelope_id {
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
                eprintln!("[nip17-rrt] bob socket error: {}", e);
                break;
            }
        }
    }
    let _ = bob_sock.close(None);

    let delivered = match delivered {
        Some(e) => e,
        None => {
            eprintln!(
                "SKIP: bob never received envelope id={} within {:?}",
                envelope_id, ROUND_TRIP_BUDGET
            );
            return;
        }
    };

    // ── Decrypt + assert plaintext matches ──────────────────────────────────
    let unwrapped =
        unwrap_gift_wrap(&bob, &delivered).expect("bob must be able to unwrap the envelope");
    assert_eq!(
        unwrapped.sender,
        alice.public_key(),
        "unwrapped sender must be alice"
    );
    assert_eq!(
        unwrapped.rumor.content, plaintext,
        "decrypted plaintext must match what alice sent"
    );
    println!(
        "[nip17-rrt] OK: bob decrypted alice's gift-wrap; plaintext={:?}",
        unwrapped.rumor.content
    );
}
