//! Real-relay smoke tests for the Pulse e2e validation app.
//!
//! These tests open real websockets to public Nostr relays and exercise the
//! substrate pieces Pulse depends on (Nip65OutboxResolver, publish, kind:1
//! round-trip). They're `#[ignore]` by default so `cargo test --workspace`
//! stays hermetic. Run explicitly:
//!
//! ```bash
//! cargo test -p nmp-testing --features test-support \
//!   --test real_relay_smoke -- --ignored --nocapture
//! ```
//!
//! ## What lands in this commit
//!
//! - **`damus_round_trip_kind1`** (spec §5 scenario 1) — generate fresh
//!   keys, connect to `wss://relay.damus.io`, publish a kind:1 with a unique
//!   sentinel nonce, REQ it back by id+author, assert the content matches.
//!   **Proves M7 + M8 over a real socket.**
//! - **`outbox_resolves_to_kind10002_writes`** (spec §5 scenario 5) —
//!   construct a Nip65OutboxResolver over an in-memory store seeded with a
//!   kind:10002 listing `nos.lol` as the sole write-relay, verify
//!   `PublishTarget::Auto` resolves to exactly nos.lol. **Proves
//!   Nip65OutboxResolver decision logic against realistic inputs.** The
//!   "verify via cross-relay REQ" assertion is deferred to scenario 5b
//!   (T66c) since it requires the publish engine wired into the actor.
//!
//! Scenarios 2/3/4/6 are tracked as T66c (subscription-rewire, NIP-77,
//! NIP-42, multi-account reactor wiring) — each requires actor-side glue
//! that doesn't exist yet (per pulse-builder commit messages).
//!
//! ## Test-relay etiquette
//!
//! - Sentinel content includes the test name + unix-ms timestamp so the
//!   relay operator can correlate any concerning patterns to this harness.
//! - Each test generates a fresh keypair via `Keys::generate()`. No reused
//!   identity, no spam — one event per test run.

use std::net::TcpStream;
use std::sync::Once;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nmp_core::publish::{OutboxResolver, PublishTarget};
use nmp_core::store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};
// Spec §271 (2026-05-25): `Nip65OutboxResolver` was moved from
// `nmp_core::publish::nip65` into `nmp-router`.
use nmp_router::Nip65OutboxResolver;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

type RelaySocket = WebSocket<MaybeTlsStream<TcpStream>>;

const DAMUS_RELAY: &str = "wss://relay.damus.io";
const NOS_LOL: &str = "wss://nos.lol";

const READ_TIMEOUT: Duration = Duration::from_millis(250);
const ROUND_TRIP_BUDGET: Duration = Duration::from_secs(10);

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

/// Build, sign, and serialize a kind:1 note via the `nostr` crate. Returns
/// `(event_id_hex, author_pubkey_hex, event_json)` ready to wrap in a Nostr
/// `["EVENT", <json>]` envelope.
fn build_kind1(content: &str) -> (String, String, String) {
    use nostr::util::JsonUtil as _;
    use nostr::{EventBuilder, Keys};
    let keys = Keys::generate();
    let event = EventBuilder::text_note(content)
        .sign_with_keys(&keys)
        .expect("sign kind:1");
    (
        event.id.to_hex(),
        event.pubkey.to_hex(),
        event.as_json(),
    )
}

/// Spec §5 scenario 1: publish a kind:1 via real socket, REQ it back, assert
/// content matches. Proves the publish wire format + relay-ack round-trip
/// without depending on the actor / publish-engine wiring.
#[test]
#[ignore = "real-relay smoke (run with --ignored)"]
fn damus_round_trip_kind1() {
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let nonce = format!("pulse-smoke-{}", unix_ms);
    let content = format!(
        "[real-relay-smoke kind:1 round-trip nonce={}]\n\
         If you're reading this on relay.damus.io: it's the nmp-testing harness, sorry for the noise.",
        nonce
    );

    let (event_id, author_hex, event_json) = build_kind1(&content);

    let mut socket = match open(DAMUS_RELAY) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("SKIP: cannot reach {}: {}", DAMUS_RELAY, e);
            return;
        }
    };

    // Publish.
    let publish = format!("[\"EVENT\",{}]", event_json);
    socket
        .send(Message::Text(publish))
        .expect("send EVENT frame");

    // Watch for OK and seed a REQ for round-trip.
    let req_id = format!("smoke-{}", &event_id[..8]);
    let req = format!(
        "[\"REQ\",\"{}\",{{\"ids\":[\"{}\"],\"authors\":[\"{}\"]}}]",
        req_id, event_id, author_hex
    );
    socket
        .send(Message::Text(req))
        .expect("send REQ frame");

    let deadline = Instant::now() + ROUND_TRIP_BUDGET;
    let mut seen_ok = false;
    let mut seen_event = false;
    while Instant::now() < deadline && (!seen_ok || !seen_event) {
        match socket.read() {
            Ok(Message::Text(text)) => {
                println!("[damus] <- {}", text.chars().take(200).collect::<String>());
                if text.contains("\"OK\"") && text.contains(&event_id) {
                    if text.contains("true") {
                        seen_ok = true;
                    } else {
                        panic!("relay rejected the publish: {}", text);
                    }
                } else if text.contains("\"EVENT\"") && text.contains(&req_id) && text.contains(&nonce) {
                    seen_event = true;
                }
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => {
                eprintln!("[damus] socket error: {}", e);
                break;
            }
        }
    }

    let _ = socket.close(None);

    assert!(seen_ok, "did not observe OK frame for published event within budget");
    assert!(
        seen_event,
        "did not observe EVENT frame round-trip via REQ within budget"
    );
}

/// Spec §5 scenario 5 (substrate slice): construct a Nip65OutboxResolver over
/// an in-memory store seeded with a kind:10002 listing exactly one write-relay
/// (`nos.lol`), assert `PublishTarget::Auto` resolves to exactly that relay.
///
/// **Proves the resolver decision logic against realistic kind:10002 shape.**
/// The full "publish lands ONLY on nos.lol, observable cross-relay" assertion
/// is deferred to T66c — it requires the publish engine wired into the actor
/// + relay manager, which is not yet built.
#[test]
#[ignore = "real-relay smoke (run with --ignored)"]
fn outbox_resolves_to_kind10002_writes() {
    use nostr::util::JsonUtil as _;
    use nostr::{EventBuilder, Keys, Kind, Tag};
    use std::sync::Arc;

    let keys = Keys::generate();
    let author_hex = keys.public_key().to_hex();

    // Build a real, fully-signed kind:10002 listing nos.lol as our only
    // write-relay. Signing it through the nostr crate ensures the resolver
    // tests the same parsing path the live store will hit.
    let kind10002 = EventBuilder::new(Kind::from_u16(10002), "")
        .tag(Tag::parse(["r", NOS_LOL, "write"]).expect("parse r-tag"))
        .sign_with_keys(&keys)
        .expect("sign kind:10002");
    let json = kind10002.as_json();
    let raw: RawEvent = serde_json::from_str(&json).expect("RawEvent decode");
    let verified = VerifiedEvent::try_from_raw(raw).expect("verify kind:10002");

    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store
        .insert(verified, &"wss://test".to_string(), 1_700_000_000_000)
        .expect("insert kind:10002");

    let resolver = Nip65OutboxResolver::with_default_fallback(store);
    let resolved = resolver.resolve(&author_hex, &[], &PublishTarget::Auto, 1);
    let resolved_urls: std::collections::BTreeSet<&str> =
        resolved.iter().map(|r| r.url.as_str()).collect();

    // Resolver must pick exactly nos.lol from the kind:10002 — NOT the
    // damus.io / nos.lol indexer fallback (which would be a 2-relay set).
    assert_eq!(
        resolved.len(),
        1,
        "expected exactly one resolved relay, got {:?}",
        resolved
    );
    assert!(
        resolved_urls.contains(NOS_LOL),
        "expected nos.lol in resolved set, got {:?}",
        resolved
    );

    // And the indexer-fallback relay is explicitly NOT included since the
    // author has a kind:10002 with at least one write-relay.
    assert!(
        !resolved_urls.contains(DAMUS_RELAY),
        "damus.io fallback should not appear when author has kind:10002"
    );

    println!(
        "[outbox] resolver picked {:?} for author {} with kind:10002 listing only {}",
        resolved, author_hex, NOS_LOL
    );
}
