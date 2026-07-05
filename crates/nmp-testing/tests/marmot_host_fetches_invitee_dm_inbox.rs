//! #3057 ROUND 6 — A fetches an invitee's kind:10050 DM-inbox on-demand.
//!
//! Round-5 root cause: A never FETCHED an invitee's kind:10050 — it only
//! fetched the KeyPackage and READ the DM cache, so a cold/reset cache aborted
//! the Welcome publish. This drives a REAL `NmpApp` (A) where B's kind:10050 is
//! ONLY on-relay (not in A's cache): A must fetch it, resolve the inbox, and
//! complete the group creation (dispatching the Welcome). On master (cache-only)
//! create_group aborts with no group; with the fetch it parks, fetches the
//! kind:10050, retries, and creates the group.

#[path = "common/mod.rs"]
mod common;

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use common::recording_relay::RecordingRelay;
use mdk_sqlite_storage::MdkSqliteStorage;
use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::ActionPayload;
use nmp_marmot::projection::action::{MarmotAction, MARMOT_ACTION_NAMESPACE};
use nmp_marmot::service::MarmotService;
use nmp_native_runtime::{dispatch_action_bytes_typed, NmpApp};
use nostr::nips::nip19::ToBech32 as _;
use nostr::util::JsonUtil as _;
use nostr::{EventBuilder, Keys, Kind, RelayUrl, Tag};

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

#[test]
fn create_group_fetches_a_cold_invitee_kind10050_and_completes() {
    static KEYRING: OnceLock<()> = OnceLock::new();
    KEYRING.get_or_init(|| {
        keyring_core::set_default_store(keyring_core::mock::Store::new().expect("mock keyring"));
    });

    let alice_keys = Keys::generate();
    let alice_hex = alice_keys.public_key().to_hex();
    let bob_keys = Keys::generate();
    let bob_hex = bob_keys.public_key().to_hex();

    // Build B's KeyPackage (injected → cached in A) and B's kind:10050
    // (ONLY served on-relay, NOT in A's cache — the thing A must fetch).
    let (bob_kp, bob_dm_list) = {
        let storage = MdkSqliteStorage::new_in_memory().expect("bob mls storage");
        let bob = MarmotService::from_storage(storage, bob_keys.clone(), Default::default());
        let kp = bob
            .publish_key_package(vec![RelayUrl::parse("wss://test.relay").unwrap()])
            .expect("bob kp");
        // B's kind:10050 advertises a wss:// DM relay (the kind:10050 parser
        // keeps only wss:// URLs). The relay it POINTS at is irrelevant to this
        // test — we assert the create_group completes once the inbox resolves,
        // not that the fire-and-forget Welcome reaches that relay.
        let dm = EventBuilder::new(Kind::from_u16(10050), "")
            .tags([Tag::custom(
                nostr::TagKind::Custom("relay".into()),
                ["wss://bob-dm-inbox.example".to_string()],
            )])
            .sign_with_keys(&bob_keys)
            .expect("sign bob kind:10050");
        (kp.event_30443, dm)
    };

    // The recording relay serves B's kind:10050 to any matching REQ.
    let mut relay = RecordingRelay::spawn(vec![bob_dm_list]);
    let relay_url = relay.url().to_string();

    // ── Boot host A: substrate + nip17 (kind:10050 parser) + marmot. ──
    let app_ptr = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    let app: &NmpApp = unsafe { &*app_ptr };
    nmp_substrate::install(
        unsafe { &mut *app_ptr },
        nmp_substrate::SubstrateConfig::default(),
    );
    // nip17 BEFORE marmot: its kind:10050 parser must run before marmot's
    // retry trigger so the DM cache is populated when the parked op retries.
    nmp_nip17::installer::register(unsafe { &mut *app_ptr }, nmp_nip17::installer::Config::default())
        .expect("nip17 register");
    let marmot_dir = tempfile::tempdir().expect("marmot dir");
    nmp_marmot::install(unsafe { &mut *app_ptr }, app.marmot_config(marmot_dir.path()))
        .expect("marmot install");
    let rx = install_update_signal(app);
    app.start_runtime(256, 8);

    app.add_relay(relay_url.clone(), "both".to_string());
    assert!(app.wait_barrier_for_test(Duration::from_secs(5)), "add relay drains");
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

    // Inject B's KeyPackage (cached) — NOT B's kind:10050 (that stays cold in
    // A's cache; only the relay has it).
    assert!(
        app.inject_signed_event_json_for_test(&bob_kp.as_json()),
        "inject bob kind:30443"
    );
    assert!(app.wait_barrier_for_test(Duration::from_secs(5)), "kp ingest drains");

    // ── A creates a group inviting B (B's kind:10050 is cold). ──
    let action = MarmotAction::CreateGroup {
        name: "round6".to_string(),
        description: String::new(),
        invitee_text: None,
        invitee_npubs: Some(vec![bob_keys.public_key().to_bech32().unwrap()]),
        signed_key_package_events_json: Vec::new(),
        relays: vec![relay_url.clone()],
    };
    let envelope = encode_dispatch_envelope(
        "round6-create-group",
        MARMOT_ACTION_NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &action.encode(),
    );
    let outcome = dispatch_action_bytes_typed(app, &envelope);
    assert_eq!(outcome.error, None, "dispatch envelope must be accepted");

    // A must actively FETCH B's kind:10050 (the round-5 bug: it never did). The
    // relay observes the author-scoped kind:10050 REQ.
    relay.wait_req("A must fetch B's kind:10050 DM-inbox", |f| {
        common::recording_relay::has_kind(f, 10050)
            && common::recording_relay::has_author(f, &bob_hex)
    });

    // ── THE GO ASSERTION: A fetches B's kind:10050 from the relay, resolves
    //    the inbox, retries the parked op, and creates the group. On master
    //    (cache-only, no fetch) create_group aborts → groups stays 0 forever. ──
    let group_deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let _ = app.wait_barrier_for_test(Duration::from_millis(500));
        if marmot_groups(app) >= 1 {
            break;
        }
        assert!(
            Instant::now() < group_deadline,
            "A never created the group — it did not fetch B's cold kind:10050 \
             DM-inbox to resolve the Welcome route (the round-5 bug). With the \
             on-demand fetch, the parked create_group must resolve + complete."
        );
        let _ = rx.recv_timeout(Duration::from_millis(300));
    }

    drop(relay);
    app.set_update_listener(None);
    unsafe { drop(Box::from_raw(app_ptr)) };
}
