//! Integration tests covering the M6 task #43 contract:
//!
//! 1. LocalKeySigner round-trips raw + ncryptsec payloads.
//! 2. LocalKeySigner sign post-condition (pubkey + id match).
//! 4. AccountManager: 3 accounts, switch flips signer_active synchronously.
//! 5. kind:3 rewire observer fires once per (non-noop) flip.
//! 6. Nip46Signer transport handshake + sign round-trip via stub transport.
//! 7. bunker:// URI parses + round-trips for a real-world example with all
//!    fields.
//!
//! Plus a zeroization wire-compatibility guard: wrapping
//! `LocalKeyMaterial::Raw` in `Zeroizing<String>` must not change the on-disk
//! JSON form.

use std::sync::{Arc, Mutex};

use nmp_signer_iface::{Nip46Rpc, Nip46Transport, SignerError, SignerOp, UnsignedEvent};
use nmp_signers::{
    parse_bunker_uri, AccountManager, ActiveChangeEvent, ActiveChangeObserver, LocalKeySigner,
    Nip46SignerHandle, Signer, SignerBackend, SignerPayload,
    Nip46Signer,
};
use nostr::nips::nip19::FromBech32;
use nostr::SecretKey;

const SAMPLE_PK: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

#[test]
fn t1_local_signer_round_trips_payloads() {
    let signer = LocalKeySigner::generate();
    let pubkey_hex = signer.pubkey().to_hex();

    // Raw round-trip.
    let payload = signer.to_payload().expect("to_payload raw");
    let SignerPayload::Local(lp) = payload else {
        panic!("expected local payload");
    };
    let restored = LocalKeySigner::from_payload(&lp).expect("from_payload raw");
    assert_eq!(restored.pubkey().to_hex(), pubkey_hex);

    // Ncryptsec round-trip.  log_n=8 keeps the test under 200ms (scrypt at
    // logn=16 is multi-second; production callers leave the default).
    let with_pwd = LocalKeySigner::from_secret_hex(&signer.secret_hex())
        .unwrap()
        .with_password(Some("hunter2".to_string()))
        .with_ncryptsec_log_n(8);
    let SignerPayload::Local(lp) = with_pwd.to_payload().expect("to_payload ncryptsec") else {
        panic!("expected local payload");
    };
    let restored = LocalKeySigner::from_payload_with_password(&lp, Some("hunter2"))
        .expect("from_payload ncryptsec");
    assert_eq!(restored.pubkey().to_hex(), pubkey_hex);

    // Wrong password fails.
    let err = LocalKeySigner::from_payload_with_password(&lp, Some("wrong"));
    assert!(err.is_err());
}

#[test]
fn t2_local_signer_sign_post_condition() {
    let signer = LocalKeySigner::generate();
    let pubkey = signer.pubkey();
    let unsigned = UnsignedEvent {
        pubkey: pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "hello nostr".to_string(),
        created_at: 1_700_000_000,
    };
    let signed = signer
        .sign(unsigned.clone())
        .wait(std::time::Duration::from_secs(1))
        .expect("sign");
    assert_eq!(signed.unsigned.pubkey, pubkey.to_hex());
    assert_eq!(signed.unsigned.content, "hello nostr");
    assert_eq!(signed.unsigned.kind, 1);
    assert!(!signed.sig.is_empty());
    assert!(!signed.id.is_empty());
}

#[test]
fn t4_three_accounts_switch_flips_signer_active() {
    let mut mgr = AccountManager::new();
    let mut pubkeys = Vec::new();
    let mut ids = Vec::new();
    for _ in 0..3 {
        let s = LocalKeySigner::generate();
        pubkeys.push(s.pubkey());
        ids.push(mgr.add(Arc::new(s)).unwrap());
    }
    assert_eq!(mgr.accounts().len(), 3);

    for (i, id) in ids.iter().enumerate() {
        mgr.switch_active(id).unwrap();
        let active_signer = mgr.signer_active().expect("active set");
        assert_eq!(
            active_signer.pubkey(),
            pubkeys[i],
            "signer_active must reflect just-set active"
        );
    }
}

/// Minimal `ActiveChangeObserver` probe. Replaces the deleted
/// `Kind3RewireObserver`, which was dead production scaffolding — active-account
/// subscription rebuilds are handled directly by the `SwitchActive` actor
/// command in `nmp-core`, never via an observer.
#[derive(Debug, Default)]
struct ProbeObserver {
    events: Mutex<Vec<ActiveChangeEvent>>,
}

impl ProbeObserver {
    fn drain(&self) -> Vec<ActiveChangeEvent> {
        match self.events.lock() {
            Ok(mut g) => std::mem::take(&mut *g),
            Err(_) => Vec::new(),
        }
    }
}

impl ActiveChangeObserver for ProbeObserver {
    fn on_active_change(&self, event: &ActiveChangeEvent) {
        if let Ok(mut g) = self.events.lock() {
            g.push(event.clone());
        }
    }
}

#[test]
fn t5_kind3_rewire_fires_per_real_switch() {
    let mut mgr = AccountManager::new();
    let obs = Arc::new(ProbeObserver::default());
    mgr.observe(obs.clone());

    let id_a = mgr.add(Arc::new(LocalKeySigner::generate())).unwrap();
    let id_b = mgr.add(Arc::new(LocalKeySigner::generate())).unwrap();

    // Three switches: A, A (no-op), B → 2 events.
    mgr.switch_active(&id_a).unwrap();
    mgr.switch_active(&id_a).unwrap();
    mgr.switch_active(&id_b).unwrap();

    let events = obs.drain();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].current.as_deref(), Some(id_a.as_str()));
    assert_eq!(events[1].current.as_deref(), Some(id_b.as_str()));
    assert_eq!(events[1].previous.as_deref(), Some(id_a.as_str()));
}

/// Stub NIP-46 transport: records every RPC, lets tests inject responses.
#[derive(Debug, Default)]
struct StubTransport {
    sent: Mutex<Vec<Nip46Rpc>>,
}

impl Nip46Transport for StubTransport {
    fn send_rpc(&self, rpc: Nip46Rpc) -> Result<(), SignerError> {
        self.sent.lock().unwrap().push(rpc);
        Ok(())
    }
}

#[test]
fn t6_nip46_handshake_and_sign_round_trip() {
    // Pre-handshake: parse bunker URI + create handle.
    let uri = format!("bunker://{SAMPLE_PK}?relay=wss://relay.example.com&secret=s1");
    let handle = Nip46SignerHandle::from_bunker_uri(&uri).expect("parse");
    assert_eq!(handle.uri().remote_pubkey_hex, SAMPLE_PK);

    // Promote to a connected signer.  In production the kernel would do the
    // `connect`/`get_public_key` handshake first; here we use a real keypair
    // we control so we can produce a valid schnorr signature for the response
    // (the mapper now runs `nostr::Event::verify()` — codex review #3).
    let remote_user_signer = LocalKeySigner::generate();
    let remote_user_pubkey = remote_user_signer.pubkey();
    let transport = Arc::new(StubTransport::default());
    let signer = handle.complete(transport.clone(), remote_user_pubkey);

    assert_eq!(signer.pubkey(), remote_user_pubkey);
    assert_eq!(signer.backend(), SignerBackend::Nip46);

    // Issue a sign call — should produce a pending op and a queued RPC.
    let unsigned = UnsignedEvent {
        pubkey: remote_user_pubkey.to_hex(),
        kind: 1,
        tags: vec![],
        content: "via bunker".to_string(),
        created_at: 1_700_000_000,
    };
    let mut op = signer.sign(unsigned.clone());

    // Verify a RPC was sent.
    let sent = transport.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    let rpc = &sent[0];
    assert!(rpc.body_json.contains("\"method\":\"sign_event\""));
    assert!(rpc.body_json.contains("\"via bunker\""));
    assert_eq!(rpc.remote_pubkey_hex, SAMPLE_PK);

    // Op should be pending.
    assert!(op.poll().is_none());

    // Build a REAL signed-event response (the mapper verifies it).
    let real_signed = remote_user_signer
        .sign(unsigned.clone())
        .wait(std::time::Duration::from_secs(1))
        .expect("real sign");
    let response_json = format!(
        r#"{{"id":"{}","pubkey":"{}","sig":"{}","kind":{},"created_at":{},"tags":[],"content":"{}"}}"#,
        real_signed.id,
        real_signed.unsigned.pubkey,
        real_signed.sig,
        real_signed.unsigned.kind,
        real_signed.unsigned.created_at,
        real_signed.unsigned.content,
    );
    signer.resolve_response(&rpc.id, Ok(response_json));

    // Poll until done (background thread converts the response).
    let signed = poll_with_timeout(&mut op, std::time::Duration::from_secs(2))
        .expect("signed event arrives");
    assert_eq!(signed.id, real_signed.id);
    assert_eq!(signed.sig, real_signed.sig);
    assert_eq!(signed.unsigned.pubkey, remote_user_pubkey.to_hex());

    // Payload round-trip.
    let payload = signer.to_payload().expect("to_payload");
    let SignerPayload::Nip46(np) = payload else {
        panic!("expected nip46 payload");
    };
    assert_eq!(np.remote_pubkey_hex, SAMPLE_PK);
    assert_eq!(np.relays, vec!["wss://relay.example.com".to_string()]);
    assert_eq!(np.secret.as_deref().map(String::as_str), Some("s1"));
    assert_eq!(
        np.cached_remote_user_pubkey_hex.as_deref(),
        Some(remote_user_pubkey.to_hex()).as_deref()
    );

    // Restore.
    let restored = Nip46Signer::from_payload(&np, transport.clone()).unwrap();
    assert_eq!(restored.pubkey(), remote_user_pubkey);
}

#[test]
fn t7_bunker_uri_full_round_trip() {
    let uri = format!(
        "bunker://{SAMPLE_PK}?relay=wss://r1.example/path&relay=wss://r2.example&secret=ABC123&perms=sign_event:1,nip04_encrypt&meta=foo"
    );
    let parsed = parse_bunker_uri(&uri).unwrap();
    assert_eq!(parsed.remote_pubkey_hex, SAMPLE_PK);
    assert_eq!(parsed.relays.len(), 2);
    assert_eq!(parsed.secret.as_deref().map(String::as_str), Some("ABC123"));
    assert_eq!(
        parsed.permissions.as_deref(),
        Some("sign_event:1,nip04_encrypt")
    );
    assert_eq!(parsed.extra.len(), 1);
    assert_eq!(parsed.extra[0], ("meta".to_string(), "foo".to_string()));

    let printed = parsed.to_string();
    let reparsed = parse_bunker_uri(&printed).unwrap();
    assert_eq!(parsed, reparsed);
}

/// Zeroization wire-compatibility guard: wrapping `LocalKeyMaterial::Raw` in
/// `Zeroizing<String>` must NOT change the on-disk JSON form. `Zeroizing<T>`
/// serializes transparently (inner value, no wrapper) via the `zeroize`
/// `serde` feature, so a payload written before this change still parses and a
/// freshly serialized payload matches the historical shape exactly.
#[test]
fn local_payload_raw_json_wire_form_unchanged_after_zeroize_wrap() {
    use nmp_signers::signers::{LocalKeyMaterial, LocalPayload};

    let signer = LocalKeySigner::generate();
    let SignerPayload::Local(lp) = signer.to_payload().expect("to_payload") else {
        panic!("expected local payload");
    };
    let LocalKeyMaterial::Raw(ref hex) = lp.key else {
        panic!("expected raw key material");
    };
    let hex = hex.to_string();

    // Serialized form: `value` is the bare hex string, not an object — proof
    // `Zeroizing` is transparent on the wire.
    let json = serde_json::to_string(&lp).expect("serialize LocalPayload");
    let expected = format!(r#"{{"key":{{"form":"raw","value":"{hex}"}}}}"#);
    assert_eq!(json, expected, "Zeroizing wrap must not alter JSON shape");

    // A payload stored before the change (plain-string form) still parses.
    let from_legacy: LocalPayload =
        serde_json::from_str(&expected).expect("parse legacy JSON form");
    let LocalKeyMaterial::Raw(ref restored) = from_legacy.key else {
        panic!("expected raw key material");
    };
    assert_eq!(restored.as_str(), hex);

    // Full round-trip: restored signer derives the same pubkey.
    let restored_signer =
        LocalKeySigner::from_payload(&from_legacy).expect("from_payload");
    assert_eq!(restored_signer.pubkey(), signer.pubkey());
}

// --- helpers -------------------------------------------------------------

fn poll_with_timeout<T: Send + 'static>(
    op: &mut SignerOp<T>,
    timeout: std::time::Duration,
) -> Result<T, SignerError> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(r) = op.poll() {
            return r;
        }
        if std::time::Instant::now() >= deadline {
            return Err(SignerError::Timeout("poll deadline".to_string()));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn nsec_parses() {
    // Sanity: nostr 0.44 nsec round-trip helper (not part of the 7, but cheap).
    let keys = nostr::Keys::generate();
    use nostr::nips::nip19::ToBech32;
    let nsec = keys.secret_key().to_bech32().unwrap();
    let parsed = SecretKey::from_bech32(&nsec).unwrap();
    assert_eq!(parsed.to_secret_hex(), keys.secret_key().to_secret_hex());
}
