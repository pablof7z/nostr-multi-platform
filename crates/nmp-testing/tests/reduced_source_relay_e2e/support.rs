mod relay;

use std::collections::BTreeSet;
use std::ffi::c_void;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use nmp_core::WireProjectionState;
use nmp_feed::{
    FeedAdmission, FeedParams, FeedRanking, FeedRender, FeedScope, FeedWindow, ListId,
    ProjectionKey,
};
use nmp_native_runtime::{FeedOpenError, NmpApp};
use nostr::{Event, EventBuilder, JsonUtil, Keys, Kind, Tag, Timestamp, ToBech32};

pub(crate) use relay::{has_author, has_kind, RecordingRelay};

pub(crate) static SERIAL: Mutex<()> = Mutex::new(());
static UPDATE_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();

pub(crate) extern "C" fn update_signal_callback(_ctx: *mut c_void, _ptr: *const u8, _len: usize) {
    if let Some(slot) = UPDATE_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(());
            }
        }
    }
}

pub(crate) fn install_update_signal() -> Receiver<()> {
    let (tx, rx) = channel();
    let slot = UPDATE_TX.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    rx
}

pub(crate) fn uninstall_update_signal() {
    if let Some(slot) = UPDATE_TX.get() {
        *slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

pub(crate) fn new_started_reduced_source_app() -> *mut NmpApp {
    let app = new_reduced_source_app_before_start();
    unsafe { &*app }.start_runtime(256, 8);
    app
}

pub(crate) fn new_reduced_source_app_before_start() -> *mut NmpApp {
    let app = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    nmp_substrate::install(
        unsafe { &mut *app },
        nmp_substrate::SubstrateConfig::default(),
    );
    unsafe { &*app }.set_update_listener(Some(std::sync::Arc::new(|bytes: &[u8]| {
        update_signal_callback(std::ptr::null_mut(), bytes.as_ptr(), bytes.len());
    })));
    app
}

pub(crate) fn start_app(app: *mut NmpApp) {
    unsafe { &*app }.start_runtime(256, 8);
}

pub(crate) fn add_relay(app: *mut NmpApp, relay: &str) {
    // SAFETY: app is a valid, non-null pointer.
    unsafe { &*app }.add_relay(relay.to_owned(), "both,indexer".to_owned());
    assert!(
        unsafe { &*app }.wait_barrier_for_test(Duration::from_millis(5_000)),
        "add relay command must drain"
    );
}

pub(crate) fn sign_in(app: *mut NmpApp, keys: &Keys) {
    let nsec = keys.secret_key().to_bech32().expect("nsec bech32");
    // SAFETY: app is a valid, non-null pointer.
    unsafe { &*app }.signin_nsec_for_test(nsec, true);
}

pub(crate) fn add_inactive_signer(app: *mut NmpApp, keys: &Keys) {
    let nsec = keys.secret_key().to_bech32().expect("nsec bech32");
    // SAFETY: app is a valid, non-null pointer.
    unsafe { &*app }.signin_nsec_for_test(nsec, false);
    assert!(
        unsafe { &*app }.wait_barrier_for_test(Duration::from_millis(5_000)),
        "inactive signer registration must drain"
    );
}

pub(crate) fn wait_active(rx: &Receiver<()>, app: &NmpApp, pubkey: &str) {
    wait_for(rx, "active account", || {
        app.active_account_handle().lock().unwrap().as_deref() == Some(pubkey)
    });
}

pub(crate) fn inject_event(app: *mut NmpApp, rx: &Receiver<()>, app_ref: &NmpApp, event: &Event) {
    let event_json = event.as_json();
    assert!(
        // SAFETY: app is a valid, non-null pointer.
        unsafe { &*app }.inject_signed_event_json_for_test(&event_json),
        "signed event must verify and inject"
    );
    assert!(
        unsafe { &*app }.wait_barrier_for_test(Duration::from_millis(5_000)),
        "actor must process injected event before the test continues"
    );
    let id = event.id.to_hex();
    wait_for(rx, "event readable", || app_ref.event_by_id(&id).is_some());
}

pub(crate) fn wait_for(rx: &Receiver<()>, label: &str, pred: impl Fn() -> bool) {
    if pred() {
        return;
    }
    loop {
        rx.recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("timed out waiting for {label}"));
        if pred() {
            return;
        }
    }
}

pub(crate) fn keys_from_byte(byte: u8) -> Keys {
    let sk = nostr::SecretKey::from_slice(&[byte; 32]).expect("valid secret");
    Keys::new(sk)
}

pub(crate) fn signed_contact_list(keys: &Keys, follows: &[String], created_at: u64) -> Event {
    let tags: Vec<Tag> = follows
        .iter()
        .map(|pk| Tag::parse(["p", pk.as_str()]).expect("valid p tag"))
        .collect();
    EventBuilder::new(Kind::from(3u16), "")
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:3")
}

pub(crate) fn signed_mute_list(keys: &Keys, muted_pubkeys: &[String], created_at: u64) -> Event {
    let tags: Vec<Tag> = muted_pubkeys
        .iter()
        .map(|pk| Tag::parse(["p", pk.as_str()]).expect("valid p tag"))
        .collect();
    EventBuilder::new(Kind::from(10_000u16), "")
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:10000")
}

pub(crate) fn signed_people_list(
    keys: &Keys,
    list_id: &str,
    members: &[String],
    created_at: u64,
) -> Event {
    let mut tags: Vec<Tag> = vec![Tag::parse(["d", list_id]).expect("valid d tag")];
    tags.extend(
        members
            .iter()
            .map(|pk| Tag::parse(["p", pk.as_str()]).expect("valid p tag")),
    );
    EventBuilder::new(Kind::from(30_000u16), "")
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:30000")
}

pub(crate) fn signed_relay_list(keys: &Keys, relays: &[&str], created_at: u64) -> Event {
    let tags: Vec<Tag> = relays
        .iter()
        .map(|url| Tag::parse(["r", *url]).expect("valid r tag"))
        .collect();
    EventBuilder::new(Kind::from(10_002u16), "")
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:10002")
}

pub(crate) fn signed_note(keys: &Keys, content: &str, created_at: u64) -> Event {
    EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign note")
}

pub(crate) fn active_follows_params(projection: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::OpCentric,
        acquisition: FeedScope::ActiveUserFollows,
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow { initial_limit: 80 },
        projection: ProjectionKey::app_owned(projection).unwrap(),
    }
}

pub(crate) fn flat_active_follows_params(projection: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::Flat,
        acquisition: FeedScope::ActiveUserFollows,
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow { initial_limit: 80 },
        projection: ProjectionKey::app_owned(projection).unwrap(),
    }
}

pub(crate) fn mute_source_params(projection: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::Flat,
        acquisition: FeedScope::ListMembers {
            list: ListId(nmp_nip51::ACTIVE_MUTE_LIST_PUBKEY_SOURCE_ID.to_string()),
        },
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow { initial_limit: 80 },
        projection: ProjectionKey::app_owned(projection).unwrap(),
    }
}

pub(crate) fn list_members_params(projection: &str, list_id: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::Flat,
        acquisition: FeedScope::ListMembers {
            list: ListId(list_id.to_string()),
        },
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow { initial_limit: 80 },
        projection: ProjectionKey::app_owned(projection).unwrap(),
    }
}

pub(crate) fn compiler(
    app: &NmpApp,
    params: &FeedParams,
    kinds: &BTreeSet<u32>,
) -> Result<nmp_feed::FeedSessionBuild, FeedOpenError> {
    nmp_native_runtime::compile_feed_params(app, params, kinds)
}

pub(crate) fn flat_feed_ids(app: &NmpApp, key: &str) -> Vec<String> {
    let Some(row) = app
        .run_typed_snapshot_projections()
        .into_iter()
        .find(|row| row.key == key && row.state != WireProjectionState::Cleared)
    else {
        return Vec::new();
    };
    nmp_note_feed::op_feed::decode_op_feed_snapshot(&row.payload)
        .expect("NNFS payload decodes")
        .cards
        .into_iter()
        .map(|card| card.card.id)
        .collect()
}

pub(crate) fn wait_feed_ids(rx: &Receiver<()>, app: &NmpApp, key: &str, expected: &[String]) {
    wait_for(rx, "feed ids", || flat_feed_ids(app, key) == expected);
}
