//! #3057 ROUND 9 (#3071) — A resolves an invitee's FRESHEST kind:10050.
//!
//! Same staleness class as #3068 (kind:30443 key packages), applied to
//! kind:10050 DM-inbox lists. A reused-identity cache accumulates multiple
//! kind:10050 for B across saga rounds; if A resolves a STALE one (pointing at
//! a dead relay), the Welcome is published to a dead relay and silently lost.
//!
//! This drives a REAL `NmpApp` (A) with substrate + nip17 (the kind:10050
//! parser/cache) + marmot: A ingests B's KeyPackage + TWO kind:10050 (an older
//! one → a dead relay, then a fresher one → a live relay). A must resolve B's
//! CURRENT inbox and COMPLETE the invite (create the group). The load-bearing
//! freshness guarantee — that a stale kind:10050 never overwrites the current
//! DM-inbox list — is proven at the cache-upsert level by the nip17 unit test
//! `stale_upsert_after_a_newer_one_is_ignored` (the kernel store's replaceable
//! gate protects the live-ingest path here, so the ordering bug only trips on
//! the reused-identity/replay path the cache keep-newest fix makes irrelevant).

#[path = "common/mod.rs"]
mod common;

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use mdk_sqlite_storage::MdkSqliteStorage;
use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::ActionPayload;
use nmp_marmot::projection::action::{MarmotAction, MARMOT_ACTION_NAMESPACE};
use nmp_marmot::service::MarmotService;
use nmp_native_runtime::{dispatch_action_bytes_typed, NmpApp};
use nostr::nips::nip19::ToBech32 as _;
use nostr::util::JsonUtil as _;
use nostr::{EventBuilder, Keys, Kind, RelayUrl, Tag, Timestamp};

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

fn marmot_groups(app: &NmpApp) -> usize {
    app.run_typed_snapshot_projections_for_test()
        .into_iter()
        .find(|p| p.key == nmp_marmot::wire::snapshot_fb::PROJECTION_KEY)
        .and_then(|p| nmp_marmot::wire::snapshot_fb::decode_marmot_snapshot(&p.payload).ok())
        .map(|s| s.groups.len())
        .unwrap_or(0)
}

fn marmot_registered(app: &NmpApp) -> bool {
    app.run_typed_snapshot_projections_for_test()
        .into_iter()
        .find(|p| p.key == nmp_marmot::wire::snapshot_fb::PROJECTION_KEY)
        .and_then(|p| nmp_marmot::wire::snapshot_fb::decode_marmot_snapshot(&p.payload).ok())
        .map(|s| s.is_registered)
        .unwrap_or(false)
}

fn dm_list(bob_keys: &Keys, relay: &str, created_at: u64) -> nostr::Event {
    EventBuilder::new(Kind::from_u16(10050), "")
        .tags([Tag::custom(
            nostr::TagKind::Custom("relay".into()),
            [relay.to_string()],
        )])
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(bob_keys)
        .expect("sign bob kind:10050")
}

#[test]
fn create_group_resolves_the_freshest_invitee_kind10050_and_completes() {
    static KEYRING: OnceLock<()> = OnceLock::new();
    KEYRING.get_or_init(|| {
        keyring_core::set_default_store(keyring_core::mock::Store::new().expect("mock keyring"));
    });

    let alice_keys = Keys::generate();
    let alice_hex = alice_keys.public_key().to_hex();
    let bob_keys = Keys::generate();

    // B's KeyPackage (cached in A) + two kind:10050: older → dead relay, newer →
    // live relay. Both advertise wss:// (the parser keeps only wss://).
    let bob_kp = {
        let storage = MdkSqliteStorage::new_in_memory().expect("bob mls storage");
        let bob = MarmotService::from_storage(storage, bob_keys.clone(), Default::default());
        bob.publish_key_package(vec![RelayUrl::parse("wss://test.relay").unwrap()])
            .expect("bob kp")
            .event_30443
    };
    let stale_dm = dm_list(&bob_keys, "wss://dead-relay.example", 1_000);
    let fresh_dm = dm_list(&bob_keys, "wss://live-relay.example", 2_000);

    // ── Boot host A: substrate + nip17 (kind:10050 parser/cache) + marmot. ──
    let app_ptr = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    let app: &NmpApp = unsafe { &*app_ptr };
    nmp_substrate::install(
        unsafe { &mut *app_ptr },
        nmp_substrate::SubstrateConfig::default(),
    );
    nmp_nip17::installer::register(unsafe { &mut *app_ptr }, nmp_nip17::installer::Config::default())
        .expect("nip17 register");
    let marmot_dir = tempfile::tempdir().expect("marmot dir");
    nmp_marmot::install(unsafe { &mut *app_ptr }, app.marmot_config(marmot_dir.path()))
        .expect("marmot install");
    let rx = install_update_signal(app);
    app.start_runtime(256, 8);

    app.signin_nsec_for_test(
        alice_keys.secret_key().to_bech32().expect("nsec").to_string(),
        true,
    );
    assert!(app.wait_barrier_for_test(Duration::from_secs(5)), "sign-in drains");
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.active_account_handle().lock().unwrap().as_deref() != Some(alice_hex.as_str()) {
        let _ = rx.recv_timeout(Duration::from_millis(250));
        assert!(Instant::now() < deadline, "A never became active");
    }
    let reg_deadline = Instant::now() + Duration::from_secs(5);
    while !marmot_registered(app) {
        assert!(Instant::now() < reg_deadline, "Marmot projection never activated");
        let _ = rx.recv_timeout(Duration::from_millis(250));
    }

    // Ingest B's KeyPackage + both kind:10050 (stale first, then fresh) through
    // the real kernel ingest → nip17 kind:10050 parser → DmRelayCache.
    for ev in [&bob_kp, &stale_dm, &fresh_dm] {
        assert!(
            app.inject_signed_event_json_for_test(&ev.as_json()),
            "inject {}",
            ev.kind
        );
    }
    assert!(app.wait_barrier_for_test(Duration::from_secs(5)), "ingest drains");

    // ── A creates a group inviting B. Its inbox must resolve (to B's current
    //    kind:10050) so the invite COMPLETES. ──
    let action = MarmotAction::CreateGroup {
        name: "round9".to_string(),
        description: String::new(),
        invitee_text: None,
        invitee_npubs: Some(vec![bob_keys.public_key().to_bech32().unwrap()]),
        signed_key_package_events_json: Vec::new(),
        relays: vec!["wss://test.relay".to_string()],
    };
    let envelope = encode_dispatch_envelope(
        "round9-create-group",
        MARMOT_ACTION_NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &action.encode(),
    );
    assert_eq!(
        dispatch_action_bytes_typed(app, &envelope).error,
        None,
        "dispatch envelope must be accepted"
    );

    let group_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let _ = app.wait_barrier_for_test(Duration::from_millis(400));
        if marmot_groups(app) >= 1 {
            break;
        }
        assert!(
            Instant::now() < group_deadline,
            "A never created the group — B's kind:10050 DM-inbox did not resolve \
             from the freshness-aware cache"
        );
        let _ = rx.recv_timeout(Duration::from_millis(300));
    }

    app.set_update_listener(None);
    unsafe { drop(Box::from_raw(app_ptr)) };
}
