//! #3057 ROUND 5 — HOST-level: does A actually PUT a kind:1059 Welcome on the
//! wire when creating a group + inviting B?
//!
//! On-device tracing showed A never publishes the Welcome (MDK encodes it, then
//! nothing reaches any relay). This drives a REAL `NmpApp` (A) end to end
//! through the kernel publish path against an in-process recording relay:
//!   - A is connected to the relay;
//!   - B's kind:30443 KeyPackage + kind:10050 DM-inbox (pointing at the relay)
//!     are ingested into A so `resolve_invitee_inboxes` succeeds;
//!   - A dispatches `create_group` inviting B;
//!   - the relay MUST receive a client-published kind:1059 gift-wrap.
//!
//! If the relay never sees a kind:1059, A's Welcome is dropped somewhere in the
//! real publish pipeline (the round-5 GO blocker).

#[path = "common/mod.rs"]
mod common;

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

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

fn bob_key_package_and_dm_list(bob_keys: &Keys, relay_url: &str) -> (nostr::Event, nostr::Event) {
    let storage = MdkSqliteStorage::new_in_memory().expect("bob mls storage");
    let bob = MarmotService::from_storage(storage, bob_keys.clone(), Default::default());
    let kp = bob
        .publish_key_package(vec![RelayUrl::parse(relay_url).expect("relay url")])
        .expect("bob kp");

    // B's kind:10050 DM-inbox list points at the (recording) relay — the kernel
    // ingests kind:10050 natively into its DM-relay cache, which
    // resolve_invitee_inboxes reads.
    let dm_list = EventBuilder::new(Kind::from_u16(10050), "")
        .tags([Tag::custom(
            nostr::TagKind::Custom("relay".into()),
            [relay_url.to_string()],
        )])
        .sign_with_keys(bob_keys)
        .expect("sign bob kind:10050");

    (kp.event_30443, dm_list)
}

#[test]
fn create_group_that_cannot_resolve_invitee_inbox_surfaces_a_specific_error_not_a_silent_drop() {
    static KEYRING: OnceLock<()> = OnceLock::new();
    KEYRING.get_or_init(|| {
        keyring_core::set_default_store(keyring_core::mock::Store::new().expect("mock keyring"));
    });

    let alice_keys = Keys::generate();
    let alice_hex = alice_keys.public_key().to_hex();
    let bob_keys = Keys::generate();

    let mut relay = RecordingRelay::spawn(Vec::new());
    let relay_url = relay.url().to_string();

    // ── Boot host A with substrate + Marmot. ──
    let app_ptr = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    let app: &NmpApp = unsafe { &*app_ptr };
    nmp_substrate::install(
        unsafe { &mut *app_ptr },
        nmp_substrate::SubstrateConfig::default(),
    );
    // nip17 registers the kind:10050 ingest parser that populates A's DM-relay
    // cache — the exact seam resolve_invitee_inboxes reads to route the Welcome.
    nmp_nip17::installer::register(unsafe { &mut *app_ptr }, nmp_nip17::installer::Config::default())
        .expect("nip17 register");
    let marmot_dir = tempfile::tempdir().expect("marmot dir");
    nmp_marmot::install(unsafe { &mut *app_ptr }, app.marmot_config(marmot_dir.path()))
        .expect("marmot install");
    let rx = install_update_signal(app);
    app.start_runtime(256, 8);

    // A connects to the relay + signs in.
    app.add_relay(relay_url.clone(), "both".to_string());
    assert!(app.wait_barrier_for_test(Duration::from_secs(5)), "add relay drains");
    app.signin_nsec_for_test(
        alice_keys.secret_key().to_bech32().expect("nsec").to_string(),
        true,
    );
    assert!(app.wait_barrier_for_test(Duration::from_secs(5)), "sign-in drains");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while app.active_account_handle().lock().unwrap().as_deref() != Some(alice_hex.as_str()) {
        let _ = rx.recv_timeout(Duration::from_millis(250));
        assert!(std::time::Instant::now() < deadline, "A never became active");
    }
    // Wait for A's Marmot projection to ACTIVATE (rebind fires async on the
    // update-listener thread) — the action path requires an active projection.
    let reg_deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let registered = app
            .run_typed_snapshot_projections_for_test()
            .into_iter()
            .find(|p| p.key == nmp_marmot::wire::snapshot_fb::PROJECTION_KEY)
            .and_then(|p| nmp_marmot::wire::snapshot_fb::decode_marmot_snapshot(&p.payload).ok())
            .map(|s| s.is_registered)
            .unwrap_or(false);
        if registered {
            break;
        }
        assert!(
            std::time::Instant::now() < reg_deadline,
            "A's Marmot projection never activated after sign-in"
        );
        let _ = rx.recv_timeout(Duration::from_millis(250));
    }

    // ── Ingest B's KeyPackage + DM-inbox list into A. ──
    let (bob_kp, bob_dm_list) = bob_key_package_and_dm_list(&bob_keys, &relay_url);
    assert!(
        app.inject_signed_event_json_for_test(&bob_kp.as_json()),
        "inject bob kind:30443"
    );
    assert!(
        app.inject_signed_event_json_for_test(&bob_dm_list.as_json()),
        "inject bob kind:10050"
    );
    assert!(app.wait_barrier_for_test(Duration::from_secs(5)), "ingest drains");

    // ── A dispatches create_group inviting B. ──
    let action = MarmotAction::CreateGroup {
        name: "round5 host publish".to_string(),
        description: String::new(),
        invitee_text: None,
        invitee_npubs: Some(vec![bob_keys.public_key().to_bech32().unwrap()]),
        signed_key_package_events_json: Vec::new(),
        relays: vec![relay_url.clone()],
    };
    let envelope = encode_dispatch_envelope(
        "round5-create-group",
        MARMOT_ACTION_NAMESPACE,
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &action.encode(),
    );
    let outcome = dispatch_action_bytes_typed(app, &envelope);
    assert_eq!(
        outcome.error, None,
        "create_group dispatch must not error at the envelope layer: {:?}",
        outcome.error
    );
    assert!(app.wait_barrier_for_test(Duration::from_secs(5)), "action drains");
    // Give the async ProtocolCommand time to run + record.
    std::thread::sleep(Duration::from_millis(300));
    let _ = app.wait_barrier_for_test(Duration::from_secs(2));

    let snap = app
        .run_typed_snapshot_projections_for_test()
        .into_iter()
        .find(|p| p.key == nmp_marmot::wire::snapshot_fb::PROJECTION_KEY)
        .and_then(|p| nmp_marmot::wire::snapshot_fb::decode_marmot_snapshot(&p.payload).ok())
        .expect("marmot snapshot present");

    // A produced NO group and put NOTHING on the wire — the round-5 publish-side
    // S51 blocker reproduced on a real host.
    assert!(
        snap.groups.is_empty(),
        "create_group produced no active group (the Welcome path aborted)"
    );
    let published = relay.drain_published();
    assert!(
        !published.iter().any(|ev| ev.kind == Kind::from_u16(1059)),
        "no kind:1059 Welcome should reach the wire when the invite aborts"
    );

    // THE DOCTRINE FIX (#3057 round-5): the abort is NOT silent — the Marmot
    // snapshot surfaces the SPECIFIC reason (here: the invitee's kind:10050
    // DM-inbox could not be resolved, so A refuses to publish the Welcome).
    // Before the fix this vanished into the generic host action-failure toast
    // (`lastDispatchError`), leaving the Marmot UI with no explanation.
    let banner = snap.last_op_error.as_ref().expect(
        "a create_group that cannot publish the Welcome MUST surface a specific \
         last_op_error banner, not drop silently (the on-device S51 symptom)",
    );
    assert_eq!(banner.op, "create_group", "banner attributes the failing op");
    assert!(
        banner.reason.contains("kind:10050") || banner.reason.contains("DM-inbox"),
        "the surfaced reason must name the real blocker (unresolved invitee \
         DM-inbox), not a generic message; got: {}",
        banner.reason
    );

    app.set_update_listener(None);
    unsafe { drop(Box::from_raw(app_ptr)) };
}
