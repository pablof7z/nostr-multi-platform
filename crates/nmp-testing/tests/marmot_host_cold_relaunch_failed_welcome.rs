//! chirp#167 / #3057 ROUND 8 — HOST-level: a cold relaunch must not re-record
//! `last_op_error` for a Welcome that already terminally failed to process.
//!
//! Companion to `marmot_host_cold_relaunch_accepted_welcome.rs` (same root
//! cause, the OTHER symptom): app #1's host lacks the KeyPackage private key
//! for a delivered Welcome, so `process_welcome` fails and the failure is
//! recorded to `last_op_error` (#3057 rounds 2/5). mdk-core persists that
//! terminal failure to its own storage. App #2 — a fresh `NmpApp` bound to
//! the SAME storage dir + identity, modeling a relaunch — has the SAME
//! gift-wrap redelivered (the local event cache replaying everything it
//! holds). Pre-fix, `process_welcome` returns mdk-core's own
//! `WelcomePreviouslyFailed` error every time, and `ingest_giftwrap`
//! unconditionally re-records it as a FRESH `last_op_error`, repopulating the
//! generic "Couldn't complete the last action" toast on EVERY cold relaunch.
//! The fix recognizes that specific error and skips re-recording it.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use mdk_core::prelude::NostrGroupConfigData;
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

fn test_relays() -> Vec<RelayUrl> {
    vec![RelayUrl::parse("wss://test.relay").unwrap()]
}

fn marmot_snapshot(app: &NmpApp) -> Option<MarmotSnapshot> {
    app.run_typed_snapshot_projections_for_test()
        .into_iter()
        .find(|p| p.key == snapshot_fb::PROJECTION_KEY)
        .and_then(|p| snapshot_fb::decode_marmot_snapshot(&p.payload).ok())
}

fn boot_and_signin(storage_dir: &std::path::Path, bob_keys: &Keys) -> (*mut NmpApp, Receiver<()>) {
    let bob_hex = bob_keys.public_key().to_hex();
    let app_ptr = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    let app: &NmpApp = unsafe { &*app_ptr };
    nmp_substrate::install(
        unsafe { &mut *app_ptr },
        nmp_substrate::SubstrateConfig::default(),
    );
    nmp_marmot::install(unsafe { &mut *app_ptr }, app.marmot_config(storage_dir))
        .expect("marmot install");
    let rx = install_update_signal(app);
    app.start_runtime(256, 8);

    app.signin_nsec_for_test(
        bob_keys.secret_key().to_bech32().expect("nsec").to_string(),
        true,
    );
    assert!(app.wait_barrier_for_test(Duration::from_secs(5)), "sign-in drains");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while app.active_account_handle().lock().unwrap().as_deref() != Some(bob_hex.as_str()) {
        let _ = rx.recv_timeout(Duration::from_millis(250));
        assert!(std::time::Instant::now() < deadline, "bob never became active");
    }
    let reg_deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if marmot_snapshot(app).map(|s| s.is_registered).unwrap_or(false) {
            break;
        }
        assert!(
            std::time::Instant::now() < reg_deadline,
            "Bob's Marmot projection never activated after sign-in"
        );
        let _ = rx.recv_timeout(Duration::from_millis(250));
    }
    (app_ptr, rx)
}

fn teardown(app_ptr: *mut NmpApp) {
    let app: &NmpApp = unsafe { &*app_ptr };
    app.set_update_listener(None);
    unsafe { drop(Box::from_raw(app_ptr)) };
}

/// A genuine kind:1059 Welcome addressed to Bob, built against a KeyPackage
/// published by a THROWAWAY store — so the private key is NEVER in Bob's real
/// (persisted) host store and `process_welcome` genuinely fails there, exactly
/// modeling the on-device drop (#3057 rounds 2/5).
fn alice_welcome_giftwrap_for_bob_with_unreachable_key(bob_keys: &Keys) -> nostr::Event {
    let alice = MarmotService::from_storage(
        mdk_sqlite_storage::MdkSqliteStorage::new_in_memory().expect("alice mls storage"),
        Keys::generate(),
        Default::default(),
    );
    let bob_publisher = MarmotService::from_storage(
        mdk_sqlite_storage::MdkSqliteStorage::new_in_memory().expect("bob throwaway mls storage"),
        bob_keys.clone(),
        Default::default(),
    );
    let bob_kp = bob_publisher
        .publish_key_package(test_relays())
        .expect("bob kp")
        .event_30443;

    let config = NostrGroupConfigData::new(
        "Round8 Host Failed".to_string(),
        "round8-host-failed".to_string(),
        None,
        None,
        None,
        test_relays(),
        vec![alice.public_key()],
    );
    let (_group, pending) = alice
        .create_group(vec![bob_kp], config)
        .expect("alice creates group");
    let rumor = pending.welcome_rumors[0].clone();
    let gift = alice
        .wrap_welcome(&bob_keys.public_key(), rumor)
        .expect("alice gift-wraps welcome");
    pending.commit().expect("alice merges create commit");
    gift
}

#[test]
fn cold_relaunch_does_not_re_record_last_op_error_for_an_already_failed_welcome() {
    static KEYRING: OnceLock<()> = OnceLock::new();
    KEYRING.get_or_init(|| {
        keyring_core::set_default_store(keyring_core::mock::Store::new().expect("mock keyring"));
    });

    let bob_keys = Keys::generate();
    let storage_dir = tempfile::tempdir().expect("marmot storage dir");
    let gift = alice_welcome_giftwrap_for_bob_with_unreachable_key(&bob_keys);
    let gift_id = gift.id.to_hex();

    // ── App #1: the Welcome genuinely fails to process — a real, first-time
    //    drop, exactly like #3057 rounds 2/5. ──
    let (app1_ptr, _rx1) = boot_and_signin(storage_dir.path(), &bob_keys);
    let app1: &NmpApp = unsafe { &*app1_ptr };

    assert!(
        app1.inject_signed_event_json_for_test(&gift.as_json()),
        "the kind:1059 gift-wrap must verify + inject"
    );
    assert!(app1.wait_barrier_for_test(Duration::from_secs(5)), "ingest drains");

    let after_first_ingest = marmot_snapshot(app1).expect("snapshot after first ingest");
    assert!(
        after_first_ingest.pending_welcomes.is_empty(),
        "an unprocessable Welcome must never appear as pending"
    );
    let first_banner = after_first_ingest
        .last_op_error
        .expect("the FIRST genuine failure MUST surface a last_op_error banner (#3057)");
    assert_eq!(first_banner.op, "welcome_ingest");
    assert_eq!(first_banner.correlation_id, gift_id, "banner keyed by the gift-wrap id");

    // ── Simulate app termination. mdk-core has persisted the Failed
    //    processed-welcome record to the SAME storage dir. ──
    teardown(app1_ptr);

    // ── App #2: fresh NmpApp, SAME storage dir + identity — a real relaunch. ──
    let (app2_ptr, _rx2) = boot_and_signin(storage_dir.path(), &bob_keys);
    let app2: &NmpApp = unsafe { &*app2_ptr };

    let pre_replay = marmot_snapshot(app2).expect("snapshot after relaunch, before replay");
    assert!(
        pre_replay.last_op_error.is_none(),
        "last_op_error is in-memory only — a fresh relaunch starts with none: {:?}",
        pre_replay.last_op_error
    );

    // ── THE LOAD-BEARING STEP: redeliver the SAME gift-wrap. mdk-core now
    //    returns `WelcomePreviouslyFailed` (it already has a Failed record for
    //    this exact wrapper event id) — pre-fix, this got re-recorded as a
    //    FRESH last_op_error, repopulating the toast on every cold relaunch. ──
    assert!(
        app2.inject_signed_event_json_for_test(&gift.as_json()),
        "the replayed kind:1059 gift-wrap must verify + inject"
    );
    assert!(app2.wait_barrier_for_test(Duration::from_secs(5)), "replay ingest drains");

    let after_replay = marmot_snapshot(app2).expect("snapshot after replay");
    assert!(
        after_replay.pending_welcomes.is_empty(),
        "a replayed, already-failed Welcome must never appear as pending"
    );
    assert!(
        after_replay.last_op_error.is_none(),
        "chirp#167: a cold-relaunch replay of an ALREADY-terminally-failed \
         Welcome must NOT re-record last_op_error (no recurring toast): {:?}",
        after_replay.last_op_error
    );

    teardown(app2_ptr);
}
