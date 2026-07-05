//! chirp#167 / #3057 ROUND 8 — HOST-level: a cold relaunch must not resurrect
//! an already-accepted Welcome as a pending invite.
//!
//! Reproduces the on-device bug end to end on a REAL `NmpApp` pair sharing
//! the SAME on-disk Marmot storage directory (the persisted MDK state a real
//! relaunch would reopen): app #1 ingests + accepts a genuine Welcome, then is
//! torn down (simulating app termination). App #2 — a FRESH `NmpApp` bound to
//! the SAME storage dir and the SAME identity, modeling the relaunch — has
//! its Welcome gift-wrap redelivered (the local event cache re-delivering
//! everything it holds on cold start). Pre-fix, `ingest_giftwrap` re-caches
//! the already-accepted Welcome into app #2's (fresh, in-memory)
//! `pending_welcomes` map, resurrecting the "1 invite" chip. The fix checks
//! persisted MDK group state (`Active` ⇒ already accepted) before re-caching.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use mdk_core::prelude::NostrGroupConfigData;
use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::ActionPayload;
use nmp_marmot::projection::action::{MarmotAction, MARMOT_ACTION_NAMESPACE};
use nmp_marmot::projection::payload::MarmotSnapshot;
use nmp_marmot::service::MarmotService;
use nmp_marmot::wire::snapshot_fb;
use nmp_native_runtime::{dispatch_action_bytes_typed, NmpApp};
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

/// Bob's persisted MLS storage path, replicated verbatim from
/// `MarmotConfig`'s defaults (`service_id = "nmp-marmot"`, `db_key_prefix =
/// "marmot-mls-state"`) — the exact coordinates `nmp_marmot::install` derives
/// from `app.marmot_config(storage_dir)` when it rebinds to Bob's identity.
/// Pre-seeding through these SAME coordinates (before Bob's host ever boots)
/// is what makes "a real relaunch reopens the same persisted state" faithful:
/// it is the identical file + keyring id a real host would open.
fn bob_db_path(storage_dir: &std::path::Path, bob_hex: &str) -> std::path::PathBuf {
    storage_dir.join(format!("marmot-mls-state-{bob_hex}.sqlite"))
}
fn bob_db_key_id(bob_hex: &str) -> String {
    format!("marmot-mls-state.{bob_hex}")
}

/// Pre-seed Bob's persisted store with his OWN published KeyPackage — so that
/// when Bob's real host later rebinds to the SAME storage coordinates, the
/// private key material is already there and `process_welcome`/`accept_welcome`
/// can genuinely succeed (mirrors `crates/nmp-marmot/src/tests/welcome_ingest.rs`'s
/// in-crate pattern of publishing and receiving through the SAME service —
/// here "the same service" means "the same on-disk db_path + db_key_id").
fn bob_publishes_own_key_package(
    storage_dir: &std::path::Path,
    bob_keys: &Keys,
) -> nostr::Event {
    let bob_hex = bob_keys.public_key().to_hex();
    let svc = MarmotService::new(
        bob_db_path(storage_dir, &bob_hex),
        "nmp-marmot",
        &bob_db_key_id(&bob_hex),
        bob_keys.clone(),
    )
    .expect("bob's persisted MLS store must open");
    svc.publish_key_package(test_relays())
        .expect("bob publishes his key package")
        .event_30443
}

/// A genuine kind:1059 Welcome gift-wrap from a throwaway Alice, addressed to
/// Bob, built against Bob's REAL (pre-seeded) key package so his real host
/// can actually process + accept it.
fn alice_welcome_giftwrap_for_bob(bob_keys: &Keys, bob_kp: nostr::Event) -> nostr::Event {
    let alice = MarmotService::from_storage(
        mdk_sqlite_storage::MdkSqliteStorage::new_in_memory().expect("alice mls storage"),
        Keys::generate(),
        Default::default(),
    );
    let config = NostrGroupConfigData::new(
        "Round8 Host".to_string(),
        "round8-host".to_string(),
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

fn marmot_snapshot(app: &NmpApp) -> Option<MarmotSnapshot> {
    app.run_typed_snapshot_projections_for_test()
        .into_iter()
        .find(|p| p.key == snapshot_fb::PROJECTION_KEY)
        .and_then(|p| snapshot_fb::decode_marmot_snapshot(&p.payload).ok())
}

/// Boot a real `NmpApp` bound to `storage_dir`, sign in as `bob_keys`, and
/// wait for the Marmot projection to activate + become the active account.
/// Returns the raw pointer (caller owns teardown) and the update-signal
/// receiver.
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

#[test]
fn cold_relaunch_does_not_resurface_an_already_accepted_welcome_as_pending() {
    static KEYRING: OnceLock<()> = OnceLock::new();
    KEYRING.get_or_init(|| {
        keyring_core::set_default_store(keyring_core::mock::Store::new().expect("mock keyring"));
    });

    let bob_keys = Keys::generate();
    let storage_dir = tempfile::tempdir().expect("marmot storage dir");

    // Pre-seed Bob's persisted store + build a genuine Welcome he can process.
    let bob_kp = bob_publishes_own_key_package(storage_dir.path(), &bob_keys);
    let gift = alice_welcome_giftwrap_for_bob(&bob_keys, bob_kp);
    let gift_id = gift.id.to_hex();

    // ── App #1: ingest + accept, exactly like S51. ──
    let (app1_ptr, rx1) = boot_and_signin(storage_dir.path(), &bob_keys);
    let app1: &NmpApp = unsafe { &*app1_ptr };

    assert!(
        app1.inject_signed_event_json_for_test(&gift.as_json()),
        "the kind:1059 gift-wrap must verify + inject"
    );
    assert!(app1.wait_barrier_for_test(Duration::from_secs(5)), "ingest drains");
    let after_ingest = marmot_snapshot(app1).expect("snapshot after ingest");
    assert_eq!(
        after_ingest.pending_welcomes.len(),
        1,
        "the Welcome must surface as pending before accept: {:?}",
        after_ingest.pending_welcomes
    );

    let action = MarmotAction::AcceptWelcome {
        welcome_id_hex: gift_id.clone(),
    };
    let envelope = encode_dispatch_envelope(
        "round8-accept",
        MARMOT_ACTION_NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &action.encode(),
    );
    let outcome = dispatch_action_bytes_typed(app1, &envelope);
    assert_eq!(
        outcome.error, None,
        "accept_welcome must not error at the envelope layer: {:?}",
        outcome.error
    );
    assert!(app1.wait_barrier_for_test(Duration::from_secs(5)), "accept drains");
    std::thread::sleep(Duration::from_millis(300));
    let _ = app1.wait_barrier_for_test(Duration::from_secs(2));

    let after_accept = marmot_snapshot(app1).expect("snapshot after accept");
    assert!(
        after_accept.pending_welcomes.is_empty(),
        "accepting must clear the pending welcome: {:?}",
        after_accept.pending_welcomes
    );
    assert_eq!(
        after_accept.groups.len(),
        1,
        "accepting must produce exactly one joined group"
    );

    // ── Simulate app termination. Storage dir survives (kept alive below). ──
    teardown(app1_ptr);
    drop(rx1);

    // ── App #2: fresh NmpApp, SAME storage dir + identity — a real relaunch. ──
    let (app2_ptr, _rx2) = boot_and_signin(storage_dir.path(), &bob_keys);
    let app2: &NmpApp = unsafe { &*app2_ptr };

    let pre_replay = marmot_snapshot(app2).expect("snapshot after relaunch, before replay");
    assert!(
        pre_replay.pending_welcomes.is_empty(),
        "a fresh relaunch must not show any pending welcome before replay: {:?}",
        pre_replay.pending_welcomes
    );
    assert_eq!(
        pre_replay.groups.len(),
        1,
        "the joined group must persist across the relaunch"
    );

    // ── THE LOAD-BEARING STEP: redeliver the SAME gift-wrap (a cold-start
    //    local-cache replay). On master this resurrects the invite. ──
    assert!(
        app2.inject_signed_event_json_for_test(&gift.as_json()),
        "the replayed kind:1059 gift-wrap must verify + inject"
    );
    assert!(app2.wait_barrier_for_test(Duration::from_secs(5)), "replay ingest drains");

    let after_replay = marmot_snapshot(app2).expect("snapshot after replay");
    assert!(
        after_replay.pending_welcomes.is_empty(),
        "chirp#167: a cold-relaunch replay of an ALREADY-ACCEPTED Welcome must \
         NOT resurrect it as a pending invite: {:?}",
        after_replay.pending_welcomes
    );
    assert_eq!(
        after_replay.groups.len(),
        1,
        "the replayed ingest must not produce a duplicate/phantom group"
    );

    teardown(app2_ptr);
}
