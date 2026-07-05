//! #3057 ROUND 4 — HOST-level delivery of a kind:1059 Welcome through the real
//! `NmpApp` kernel ingest + parser dispatch.
//!
//! Rounds 2-3 (and the crate-internal round-4 dispatcher test) prove the ingest
//! LOGIC and the parser-dispatch wiring in isolation. This proves the link end
//! to end on a REAL host: a genuine kind:1059 Welcome addressed to the
//! signed-in identity, injected through the production kernel ingest path
//! (`inject_signed_event_json_for_test` → `ingest_pre_verified_event` →
//! `project_accepted_event`/`dispatch_at_source`), must reach the
//! host-registered `MarmotIngestParser` → `ingest_giftwrap` and surface in the
//! HOST's Marmot snapshot projection.
//!
//! Probe design: the host's Marmot store does NOT hold the KeyPackage private
//! key for the Welcome (it was published by a throwaway store), so
//! `process_welcome` fails — and #3060 routes that failure to the snapshot's
//! `last_op_error` banner with op `"welcome_ingest"`. That banner is therefore
//! a precise witness that the Welcome REACHED the ingest handler. If instead the
//! snapshot shows NO pending welcome AND NO `welcome_ingest` banner, the kind:1059
//! never reached `ingest_giftwrap` — i.e. the kernel→parser event-tap wiring is
//! the S51 break (the on-device symptom: lands on the relay, climbing event
//! count, but silent).

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use mdk_core::prelude::NostrGroupConfigData;
use mdk_sqlite_storage::MdkSqliteStorage;
use nmp_marmot::projection::payload::MarmotSnapshot;
use nmp_marmot::service::MarmotService;
use nmp_marmot::wire::snapshot_fb;
use nmp_native_runtime::NmpApp;
use nostr::nips::nip19::ToBech32 as _;
use nostr::util::JsonUtil as _;
use nostr::{Keys, RelayUrl};

static UPDATE_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();

fn install_update_signal(app: &NmpApp) -> Receiver<()> {
    let (tx, rx) = channel::<()>();
    let slot = UPDATE_TX.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
    app.set_update_listener(Some(std::sync::Arc::new(|_bytes: &[u8]| {
        if let Some(slot) = UPDATE_TX.get() {
            if let Ok(guard) = slot.lock() {
                if let Some(tx) = guard.as_ref() {
                    let _ = tx.send(());
                }
            }
        }
    })));
    rx
}

fn in_memory_service(keys: Keys) -> MarmotService {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

fn test_relays() -> Vec<RelayUrl> {
    vec![RelayUrl::parse("wss://test.relay").unwrap()]
}

/// Build a genuine kind:1059 Welcome gift-wrap addressed to `bob_keys` — the
/// exact wire artifact the kernel would deliver on B's gift-wrap inbox REQ.
fn welcome_giftwrap_for(bob_keys: &Keys) -> nostr::Event {
    let alice_keys = Keys::generate();
    let alice = in_memory_service(alice_keys.clone());
    // Throwaway B-publisher store: the KeyPackage private key lands THERE, not
    // in the host's Marmot store — so host `process_welcome` fails and #3060's
    // banner fires, which is exactly the witness we probe for.
    let bob_publisher = in_memory_service(bob_keys.clone());
    let kp = bob_publisher
        .publish_key_package(test_relays())
        .expect("bob kp");

    let config = NostrGroupConfigData::new(
        "Round4 Host".into(),
        "round4-host".into(),
        None,
        None,
        None,
        test_relays(),
        vec![alice_keys.public_key()],
    );
    let (_g, pending) = alice
        .create_group(vec![kp.event_30443.clone()], config)
        .expect("alice creates group");
    let gift = alice
        .wrap_welcome(&bob_keys.public_key(), pending.welcome_rumors[0].clone())
        .expect("gift-wrap welcome");
    pending.commit().expect("commit");
    gift
}

fn marmot_snapshot(app: &NmpApp) -> Option<MarmotSnapshot> {
    app.run_typed_snapshot_projections_for_test()
        .into_iter()
        .find(|p| p.key == snapshot_fb::PROJECTION_KEY)
        .and_then(|p| snapshot_fb::decode_marmot_snapshot(&p.payload).ok())
}

#[test]
fn kind1059_welcome_injected_into_host_reaches_marmot_ingest() {
    // The host installs the REAL encrypted keyring-backed MLS store; under
    // `cargo test` there is no platform keyring, so stand up the mock store
    // (same as MDK's own tests) — otherwise `MarmotService::new` fails and the
    // projection never activates (a test-env artifact, not the code path under
    // test).
    static KEYRING: OnceLock<()> = OnceLock::new();
    KEYRING.get_or_init(|| {
        keyring_core::set_default_store(keyring_core::mock::Store::new().expect("mock keyring"));
    });

    let bob_keys = Keys::generate();
    let bob_hex = bob_keys.public_key().to_hex();

    // ── Boot a real host with substrate + Marmot installed. ──
    let app_ptr = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    let app: &NmpApp = unsafe { &*app_ptr };
    nmp_substrate::install(
        unsafe { &mut *app_ptr },
        nmp_substrate::SubstrateConfig::default(),
    );
    let marmot_dir = tempfile::tempdir().expect("marmot storage dir");
    nmp_marmot::install(unsafe { &mut *app_ptr }, app.marmot_config(marmot_dir.path()))
        .expect("marmot install");
    let rx = install_update_signal(app);
    app.start_runtime(256, 8);

    // ── Sign in as B → rebinds the Marmot projection to B's identity. ──
    let nsec = bob_keys
        .secret_key()
        .to_bech32()
        .expect("nsec")
        .to_string();
    app.signin_nsec_for_test(nsec, true);
    assert!(
        app.wait_barrier_for_test(Duration::from_secs(5)),
        "sign-in must drain"
    );
    // Wait for B to be the active account (Marmot rebinds on the identity change).
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while app.active_account_handle().lock().unwrap().as_deref() != Some(bob_hex.as_str()) {
        let _ = rx.recv_timeout(Duration::from_millis(500));
        assert!(
            std::time::Instant::now() < deadline,
            "B never became the active account"
        );
    }

    // The Marmot projection rebinds on the identity-change observer, which
    // fires on the update-listener thread (async from the actor). Poll for it
    // to ACTIVATE. This activation is load-bearing: `MarmotIngestParser`
    // silently no-ops when `runtime.projection()` is None, so a delivered
    // Welcome is dropped without a trace whenever the projection is inactive
    // (e.g. the encrypted MLS store fails to open). Sign-in MUST activate it.
    let reg_deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if marmot_snapshot(app)
            .map(|s| s.is_registered)
            .unwrap_or(false)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < reg_deadline,
            "Marmot projection never activated after sign-in — an inactive \
             projection silently drops every delivered Welcome (no ingest, no \
             error), which is the on-device S51 silence."
        );
        let _ = rx.recv_timeout(Duration::from_millis(250));
    }

    // Sanity: before delivery there is no pending welcome and no banner.
    let pre = marmot_snapshot(app).expect("snapshot");
    assert!(pre.pending_welcomes.is_empty(), "no pending welcome yet");
    assert!(pre.last_op_error.is_none(), "no error yet");

    // ── Deliver a genuine kind:1059 Welcome addressed to B through the real
    //    kernel ingest path (simulating relay delivery on B's inbox REQ). ──
    let gift = welcome_giftwrap_for(&bob_keys);
    let gift_id = gift.id.to_hex();
    assert!(
        app.inject_signed_event_json_for_test(&gift.as_json()),
        "the kind:1059 gift-wrap must verify + inject"
    );
    assert!(
        app.wait_barrier_for_test(Duration::from_secs(5)),
        "actor must process the injected Welcome"
    );

    // The kernel must have accepted + stored the kind:1059 (sig-only admission;
    // 1059 is a stored regular kind), which is the precondition for the
    // post-store parser dispatch.
    assert!(
        app.event_by_id(&gift_id).is_some(),
        "the kernel must accept + store the kind:1059 (precondition for dispatch)"
    );

    // ── Assert the Welcome REACHED the ingest handler through the real host. ──
    let snap = marmot_snapshot(app).expect("marmot snapshot must be present after sign-in");

    // The host's Marmot store does not hold the KeyPackage private key (it was
    // published by a throwaway store), so `process_welcome` fails and #3060
    // routes it to the `welcome_ingest` banner — a precise witness that the
    // Welcome flowed kernel-ingest → dispatch → MarmotIngestParser →
    // ingest_giftwrap → process_welcome. (A pending welcome would also count,
    // but the missing key makes the banner the deterministic outcome here.)
    let reached_ingest = !snap.pending_welcomes.is_empty()
        || snap
            .last_op_error
            .as_ref()
            .map(|e| e.op == "welcome_ingest" && e.correlation_id == gift_id)
            .unwrap_or(false);

    assert!(
        reached_ingest,
        "a kind:1059 Welcome delivered through the real host kernel ingest MUST \
         reach MarmotIngestParser → ingest_giftwrap. It reached neither a \
         pending welcome nor a #3060 welcome_ingest banner — the host \
         kernel→parser event-tap wiring is broken. pending_welcomes={:?} \
         last_op_error={:?}",
        snap.pending_welcomes, snap.last_op_error
    );

    app.set_update_listener(None);
    unsafe { drop(Box::from_raw(app_ptr)) };
}
