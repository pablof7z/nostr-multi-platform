//! TEMP diagnostic — DELETE before PR. Proves the F-02 fix against a real
//! `wss://` relay (relay.damus.io), since `nak serve` is `ws://`-only and the
//! `Kind10050Parser` correctly rejects non-`wss://` DM relays.

use std::ffi::{CStr, CString};
use std::sync::Arc;
use std::time::Duration;

use nmp_ffi::{
    nmp_app_add_relay, nmp_app_free, nmp_app_free_string, nmp_app_new,
    nmp_app_read_projection_json, nmp_app_signin_nsec, nmp_app_start, NmpApp,
};
use nmp_nip59::{gift_wrap_with_signer, SignerForSeal, GIFT_WRAP_TOTAL_TIMEOUT};
use nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK;
use nostr::util::JsonUtil as _;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp, ToBech32 as _};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message};

const RELAY: &str = "wss://relay.primal.net";

fn publish(json: &str) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (mut sock, _) = connect(RELAY).expect("connect");
    if let MaybeTlsStream::Rustls(s) = sock.get_mut() {
        let _ = s.get_ref().set_read_timeout(Some(Duration::from_millis(250)));
    }
    sock.send(Message::Text(format!("[\"EVENT\",{json}]"))).expect("send");
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        if let Ok(Message::Text(t)) = sock.read() {
            if t.contains("\"OK\"") { println!("  ack: {t}"); break; }
        }
    }
    let _ = sock.close(None);
}

fn read_proj(app: *mut NmpApp, key: &str) -> String {
    let k = CString::new(key).unwrap();
    let ptr = nmp_app_read_projection_json(app, k.as_ptr());
    if ptr.is_null() { return "<null>".into(); }
    let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
    nmp_app_free_string(ptr);
    s
}

#[test]
#[ignore]
fn diag_damus() {
    let alice = Keys::generate();
    let bob = Keys::generate();
    let bob_nsec = bob.secret_key().to_bech32().unwrap();
    println!("alice={} bob={}", alice.public_key().to_hex(), bob.public_key().to_hex());

    let bob_10050 = EventBuilder::new(Kind::from_u16(10050), "")
        .tag(Tag::custom(nostr::TagKind::custom("relay"), [RELAY.to_string()]))
        .custom_created_at(Timestamp::now())
        .sign_with_keys(&bob).unwrap();
    println!("publish bob 10050:");
    publish(&bob_10050.as_json());

    let rumor = EventBuilder::new(Kind::from_u16(14), "diag hello damus")
        .tag(Tag::public_key(bob.public_key()))
        .custom_created_at(Timestamp::now())
        .build(alice.public_key());
    let signer: Arc<dyn SignerForSeal> = Arc::new(alice.clone());
    let env = gift_wrap_with_signer(&signer, &bob.public_key(), &rumor, Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK))
        .wait(GIFT_WRAP_TOTAL_TIMEOUT).unwrap();
    println!("publish alice 1059:");
    publish(&env.as_json());

    let app = nmp_app_new();
    nmp_app_template::register_defaults(unsafe { &mut *app });
    nmp_app_start(app, 0, 256, 8);
    let r = CString::new(RELAY).unwrap();
    let role = CString::new("both,indexer").unwrap();
    nmp_app_add_relay(app, r.as_ptr(), role.as_ptr());
    let nsec = CString::new(bob_nsec).unwrap();
    nmp_app_signin_nsec(app, nsec.as_ptr(), 1);

    let mut found = false;
    for i in 0..25 {
        std::thread::sleep(Duration::from_secs(1));
        let inbox = read_proj(app, "nmp.nip17.dm_inbox");
        println!("--- t={i}s --- dm_relay_list: {} | dm_inbox: {}",
            read_proj(app, "nmp.nip17.dm_relay_list"), inbox);
        if inbox.contains("diag hello damus") { found = true; println!(">>> DM RECEIVED at t={i}s"); break; }
    }
    nmp_app_free(app);
    assert!(found, "DM never surfaced");
}
