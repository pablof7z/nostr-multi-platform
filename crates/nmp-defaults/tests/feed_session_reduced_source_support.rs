use std::ffi::{c_void, CString};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use nmp_core::WireProjectionState;
use nmp_feed::{
    FeedAdmission, FeedParams, FeedRanking, FeedRender, FeedScope, FeedWindow, ListId,
    ProjectionKey,
};
pub(crate) use nmp_ffi::{
    nmp_app_free, nmp_app_set_update_callback, nmp_app_start, FeedOpenError, NmpApp,
};
use nmp_ffi::{nmp_app_inject_signed_event_json, nmp_app_signin_nsec, nmp_app_wait_barrier};
use nostr::prelude::*;
use nostr::{EventBuilder, Kind, Tag, Timestamp};

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
    let (tx, rx) = channel::<()>();
    let slot = UPDATE_TX.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap() = Some(tx);
    rx
}

pub(crate) fn uninstall_update_signal() {
    if let Some(slot) = UPDATE_TX.get() {
        *slot.lock().unwrap() = None;
    }
}

pub(crate) fn wait_for(rx: &Receiver<()>, label: &str, pred: impl Fn() -> bool) {
    if pred() {
        return;
    }
    loop {
        rx.recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("timed out waiting for {label}"));
        if pred() {
            return;
        }
    }
}

pub(crate) fn keys_from_byte(byte: u8) -> Keys {
    let sk = SecretKey::from_slice(&[byte; 32]).expect("valid secret");
    Keys::new(sk)
}

pub(crate) fn sign_in(app: *mut NmpApp, keys: &Keys) {
    let nsec = keys.secret_key().to_bech32().expect("nsec bech32");
    let secret = CString::new(nsec).expect("nsec has no nul");
    nmp_app_signin_nsec(app, secret.as_ptr(), 1);
}

pub(crate) fn wait_active(rx: &Receiver<()>, app: &NmpApp, pubkey: &str) {
    wait_for(rx, "active account", || {
        app.active_account_handle().lock().unwrap().as_deref() == Some(pubkey)
    });
}

pub(crate) fn inject_event(
    app: *mut NmpApp,
    rx: &Receiver<()>,
    app_ref: &NmpApp,
    id: &str,
    json: &str,
) {
    let event = CString::new(json).expect("event json has no nul");
    assert!(
        nmp_app_inject_signed_event_json(app, event.as_ptr()),
        "signed event must verify and inject"
    );
    assert!(
        nmp_app_wait_barrier(app, 5_000),
        "actor must process injected event before the test continues"
    );
    wait_for(rx, "event readable", || app_ref.event_by_id(id).is_some());
}

pub(crate) fn signed_people_list(
    keys: &Keys,
    list_id: &str,
    members: &[String],
    created_at: u64,
) -> (String, String) {
    let mut tags = vec![Tag::parse(["d", list_id]).expect("valid d tag")];
    tags.extend(
        members
            .iter()
            .map(|pk| Tag::parse(["p", pk.as_str()]).expect("valid p tag")),
    );
    let event = EventBuilder::new(Kind::from(30_000u16), "")
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:30000");
    (event.id.to_hex(), event.as_json())
}

pub(crate) fn signed_mute_list(
    keys: &Keys,
    muted_pubkeys: &[String],
    created_at: u64,
) -> (String, String) {
    let tags: Vec<Tag> = muted_pubkeys
        .iter()
        .map(|pk| Tag::parse(["p", pk.as_str()]).expect("valid p tag"))
        .collect();
    let event = EventBuilder::new(Kind::from(10_000u16), "")
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:10000");
    (event.id.to_hex(), event.as_json())
}

pub(crate) fn signed_contact_list(
    keys: &Keys,
    follows: &[String],
    created_at: u64,
) -> (String, String) {
    let tags: Vec<Tag> = follows
        .iter()
        .map(|pk| Tag::parse(["p", pk.as_str()]).expect("valid p tag"))
        .collect();
    let event = EventBuilder::new(Kind::from(3u16), "")
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:3");
    (event.id.to_hex(), event.as_json())
}

pub(crate) fn signed_note(keys: &Keys, content: &str, created_at: u64) -> (String, String) {
    let event = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign note");
    (event.id.to_hex(), event.as_json())
}

pub(crate) fn list_params(projection: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::Flat,
        acquisition: FeedScope::ListMembers {
            list: ListId("team".to_string()),
        },
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow { initial_limit: 80 },
        projection: ProjectionKey(projection.into()),
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
        projection: ProjectionKey(projection.into()),
    }
}

pub(crate) fn active_follows_params(projection: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::OpCentric,
        acquisition: FeedScope::ActiveUserFollows,
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow { initial_limit: 80 },
        projection: ProjectionKey(projection.into()),
    }
}

pub(crate) fn compiler(
    app: &NmpApp,
    params: &FeedParams,
    kinds: &std::collections::BTreeSet<u32>,
) -> Result<nmp_feed::FeedSessionBuild, FeedOpenError> {
    nmp_defaults::compile_feed_params(app, params, kinds)
}

pub(crate) fn flat_feed_ids(app: &NmpApp, key: &str) -> Vec<String> {
    let Some(row) = app
        .run_typed_snapshot_projections()
        .into_iter()
        .find(|row| row.key == key && row.state != WireProjectionState::Cleared)
    else {
        return Vec::new();
    };
    nmp_nip01::op_feed::decode_op_feed_snapshot(&row.payload)
        .expect("NOFS payload decodes")
        .cards
        .into_iter()
        .map(|card| card.card.id)
        .collect()
}

pub(crate) fn wait_feed_ids(rx: &Receiver<()>, app: &NmpApp, key: &str, expected: &[String]) {
    wait_for(rx, "feed ids", || flat_feed_ids(app, key) == expected);
}
