//! TEMP diagnostic — DELETE before PR. Dumps dm_relay_list / dm_inbox /
//! routing decisions every second to locate where the cold-start kernel chain
//! breaks.

use std::ffi::{CStr, CString};
use std::sync::Arc;
use std::time::Duration;

use nmp_ffi::{
    nmp_app_add_relay, nmp_app_free, nmp_app_free_string, nmp_app_new,
    nmp_app_read_projection_json, nmp_app_recent_routing_decisions, nmp_app_signin_nsec,
    nmp_app_start, NmpApp,
};
use nmp_nip59::{gift_wrap_with_signer, SignerForSeal, GIFT_WRAP_TOTAL_TIMEOUT};
use nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK;
use nostr::util::JsonUtil as _;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp, ToBech32 as _};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message};

const NAK_PORT: u16 = 10567;

fn publish(relay_url: &str, json: &str) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (mut sock, _) = connect(relay_url).expect("connect");
    if let MaybeTlsStream::Plain(s) = sock.get_mut() {
        let _ = s.set_read_timeout(Some(Duration::from_millis(250)));
    }
    sock.send(Message::Text(format!("[\"EVENT\",{json}]"))).expect("send");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match sock.read() {
            Ok(Message::Text(t)) if t.contains("\"OK\"") => { println!("  ack: {t}"); break; }
            _ => {}
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

fn routing(app: *mut NmpApp) -> String {
    let ptr = nmp_app_recent_routing_decisions(app);
    if ptr.is_null() { return "<null>".into(); }
    let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
    nmp_app_free_string(ptr);
    s
}

#[test]
#[ignore]
fn diag() {
    let log = std::fs::File::create("/tmp/nak_diag_log.txt").unwrap();
    let log2 = log.try_clone().unwrap();
    let mut nak = std::process::Command::new("nak")
        .args(["serve", "--port", &NAK_PORT.to_string()])
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log2))
        .spawn()
        .expect("nak");
    std::thread::sleep(Duration::from_millis(500));
    let relay_url = format!("ws://localhost:{NAK_PORT}");

    let alice = Keys::generate();
    let bob = Keys::generate();
    let bob_nsec = bob.secret_key().to_bech32().unwrap();
    println!("alice={} bob={}", alice.public_key().to_hex(), bob.public_key().to_hex());

    let bob_10050 = EventBuilder::new(Kind::from_u16(10050), "")
        .tag(Tag::custom(nostr::TagKind::custom("relay"), [relay_url.clone()]))
        .custom_created_at(Timestamp::now())
        .sign_with_keys(&bob).unwrap();
    println!("publish bob 10050:");
    publish(&relay_url, &bob_10050.as_json());

    let rumor = EventBuilder::new(Kind::from_u16(14), "diag hello")
        .tag(Tag::public_key(bob.public_key()))
        .custom_created_at(Timestamp::now())
        .build(alice.public_key());
    let signer: Arc<dyn SignerForSeal> = Arc::new(alice.clone());
    let env = gift_wrap_with_signer(&signer, &bob.public_key(), &rumor, Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK))
        .wait(GIFT_WRAP_TOTAL_TIMEOUT).unwrap();
    println!("publish alice 1059:");
    publish(&relay_url, &env.as_json());

    // Inbound-frame sniffer: records every text frame the kernel receives from
    // the relay so we can see whether Bob's kind:10050 / the gift-wrap arrive.
    struct Sniffer;
    impl nmp_core::substrate::RelayTextInterceptor for Sniffer {
        fn on_relay_text(
            &self,
            _kernel: &mut nmp_core::Kernel,
            relay_url: &str,
            text: &str,
        ) -> Vec<nmp_core::OutboundMessage> {
            let short = if text.len() > 120 { &text[..120] } else { text };
            println!("INBOUND [{relay_url}]: {short}");
            Vec::new()
        }
    }

    let app = nmp_app_new();
    unsafe { &*app }.add_relay_text_interceptor(std::sync::Arc::new(Sniffer));
    nmp_app_template::register_defaults(unsafe { &mut *app });
    nmp_app_start(app, 0, 256, 8);
    let r = CString::new(relay_url.as_str()).unwrap();
    let role = CString::new("both,indexer").unwrap();
    nmp_app_add_relay(app, r.as_ptr(), role.as_ptr());
    let nsec = CString::new(bob_nsec).unwrap();
    nmp_app_signin_nsec(app, nsec.as_ptr(), 1);

    for i in 0..8 {
        std::thread::sleep(Duration::from_secs(1));
        println!("--- t={i}s --- dm_inbox: {}", read_proj(app, "nmp.nip17.dm_inbox"));
    }
    println!("routing: {}", routing(app));

    nmp_app_free(app);
    let _ = nak.kill();
    let _ = nak.wait();
    println!("=== NAK LOG (REQs the kernel sent) ===");
    println!("{}", std::fs::read_to_string("/tmp/nak_diag_log.txt").unwrap_or_default());
}
