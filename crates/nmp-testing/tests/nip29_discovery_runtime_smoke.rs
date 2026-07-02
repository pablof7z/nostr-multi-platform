//! Hermetic public-runtime smoke for NIP-29 discovery.
//!
//! A local WebSocket relay records the actual `REQ` frames and serves a signed
//! kind:39000 group-metadata event. The test drives only public app-facing
//! NMP surfaces: crate-level `nmp_nip29::register`, the native runtime
//! discovery session, and the typed dispatch envelope for `nmp.nip29.discover`.

#[path = "common/mod.rs"]
mod common;

use std::ffi::c_void;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use common::recording_relay::{has_kind, RecordingRelay};
use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::ActionPayload;
use nmp_native_runtime::{
    dispatch_action_bytes_typed, Nip29GroupDiscoverySession, NmpApp, NmpAppBuilder, RunConfig,
};
use nmp_nip29::action::DiscoverGroupsInput;
use nmp_nip29::DiscoveredGroup;
use nostr::{Event, EventBuilder, Keys, Kind, Tag, Timestamp};

static SERIAL: Mutex<()> = Mutex::new(());
static UPDATE_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();

extern "C" fn update_signal_callback(_ctx: *mut c_void, _ptr: *const u8, _len: usize) {
    if let Some(slot) = UPDATE_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(());
            }
        }
    }
}

struct DiscoveryApp {
    app: *mut NmpApp,
    ticks: Receiver<()>,
}

impl DiscoveryApp {
    fn boot(relay_url: &str) -> Self {
        let mut builder = NmpAppBuilder::new();
        nmp_substrate::install(&mut builder, nmp_substrate::SubstrateConfig::default());
        nmp_nip29::register(&mut builder, nmp_nip29::Config::default())
            .expect("nmp-nip29 registration must not collide");

        let app = builder
            .in_memory()
            .consume_all_builtin_projections()
            .with_relays([(relay_url, "both")])
            .start(RunConfig {
                visible_limit: 256,
                emit_hz: 8,
            });

        let (tx, ticks) = channel::<()>();
        let slot = UPDATE_TX.get_or_init(|| Mutex::new(None));
        *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
        unsafe { &*app }.set_update_listener(Some(std::sync::Arc::new(|bytes: &[u8]| {
            update_signal_callback(std::ptr::null_mut(), bytes.as_ptr(), bytes.len());
        })));

        Self { app, ticks }
    }
}

impl Drop for DiscoveryApp {
    fn drop(&mut self) {
        unsafe { &*self.app }.set_update_listener(None);
        if let Some(slot) = UPDATE_TX.get() {
            if let Ok(mut guard) = slot.lock() {
                *guard = None;
            }
        }
        unsafe { drop(Box::from_raw(self.app)) };
    }
}

#[test]
fn discovery_session_dispatch_receives_group_metadata_from_relay() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let group = signed_group_metadata("room", "Room");
    let mut relay = RecordingRelay::spawn(vec![group]);
    let app = DiscoveryApp::boot(relay.url());
    let app_ref = unsafe { &*app.app };

    let (handle, reader) = app_ref.open_nip29_group_discovery_session_with_reader(
        Nip29GroupDiscoverySession::new(relay.url().to_string()),
    );
    let payload = DiscoverGroupsInput {
        relay_url: relay.url().to_string(),
    }
    .encode();
    let envelope = encode_dispatch_envelope(
        "nip29-discovery-hermetic-smoke",
        "nmp.nip29.discover",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &payload,
    );
    let outcome = dispatch_action_bytes_typed(app_ref, &envelope);
    assert_eq!(outcome.error, None);
    assert_eq!(
        outcome.correlation_id.as_deref(),
        Some("nip29-discovery-hermetic-smoke")
    );

    relay.wait_req("NIP-29 discovery metadata REQ", |filter| {
        has_kind(filter, 39_000)
            && has_kind(filter, 39_001)
            && has_kind(filter, 39_002)
            && filter.get("#d").is_none()
    });

    let groups = wait_for_groups(
        &app.ticks,
        || reader.snapshot().groups,
        Duration::from_secs(10),
    );
    app_ref.close_nip29_group_discovery_session(handle);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].group_id, "room");
    assert_eq!(groups[0].host_relay_url, relay.url());
    assert_eq!(groups[0].name.as_deref(), Some("Room"));
}

fn signed_group_metadata(group_id: &str, name: &str) -> Event {
    EventBuilder::new(Kind::from(39_000u16), "")
        .tags([
            Tag::parse(["d", group_id]).expect("valid d tag"),
            Tag::parse(["name", name]).expect("valid name tag"),
        ])
        .custom_created_at(Timestamp::from_secs(100))
        .sign_with_keys(&Keys::generate())
        .expect("sign group metadata")
}

fn wait_for_groups(
    ticks: &Receiver<()>,
    snapshot: impl Fn() -> Vec<DiscoveredGroup>,
    budget: Duration,
) -> Vec<DiscoveredGroup> {
    let mut last = snapshot();
    if !last.is_empty() {
        return last;
    }
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match ticks.recv_timeout(remaining.min(Duration::from_secs(1))) {
            Ok(()) => {
                last = snapshot();
                if !last.is_empty() {
                    return last;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return last,
        }
    }
    last
}
