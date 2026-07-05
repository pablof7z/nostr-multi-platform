//! Real relay smoke for NIP-29 discovery through the public concept path.
//!
//! This test opens the same Rust-facing surface a consumer app uses against the
//! native runtime host: `open_nip29_group_discovery_session_with_reader`, then
//! dispatches the typed `nmp.nip29.discover` action. The assertion reads the
//! returned `DiscoveredGroupsProjection`; it never injects events by hand.

#[path = "real_relay_common/mod.rs"]
mod common;

use std::ffi::c_void;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::ActionPayload;
use nmp_native_runtime::{dispatch_action_bytes_typed, NmpApp, NmpAppBuilder, RunConfig};
use nmp_nip29::action::DiscoverGroupsInput;
use nmp_nip29::{
    close_nip29_group_discovery_session, open_nip29_group_discovery_session_with_reader,
    DiscoveredGroup, Nip29GroupDiscoverySession,
};

const NIP29_RELAY: &str = "wss://nip29.f7z.io";
const DISCOVERY_BUDGET: Duration = Duration::from_secs(45);
const PROBE_BUDGET: Duration = Duration::from_secs(10);

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
    fn boot() -> Self {
        let mut builder = NmpAppBuilder::new();
        nmp_substrate::install(&mut builder, nmp_substrate::SubstrateConfig::default());
        nmp_nip29::register(&mut builder, nmp_nip29::Config::default())
            .expect("nmp-nip29 registration must not collide");

        let app = builder
            .in_memory()
            .consume_all_builtin_projections()
            .with_relays([(NIP29_RELAY, "both")])
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
#[ignore = "opens a live websocket to wss://nip29.f7z.io"]
fn nip29_discovery_session_receives_live_group_rows() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    if !relay_has_group_metadata() {
        eprintln!("SKIP: {NIP29_RELAY} did not return kind:39000 during direct probe");
        return;
    }

    let app = DiscoveryApp::boot();
    let app_ref = unsafe { &*app.app };
    let (handle, reader) = open_nip29_group_discovery_session_with_reader(
        app_ref,
        Nip29GroupDiscoverySession::new(vec![NIP29_RELAY.to_string()]),
    );

    let payload = DiscoverGroupsInput {
        relay_url: NIP29_RELAY.to_string(),
    }
    .encode();
    let envelope = encode_dispatch_envelope(
        "nip29-discovery-live-smoke",
        "nmp.nip29.discover",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &payload,
    );
    let outcome = dispatch_action_bytes_typed(app_ref, &envelope);
    assert_eq!(outcome.error, None);
    assert_eq!(
        outcome.correlation_id.as_deref(),
        Some("nip29-discovery-live-smoke")
    );

    let groups = wait_for_groups(&app.ticks, || reader.snapshot().groups, DISCOVERY_BUDGET);
    assert!(close_nip29_group_discovery_session(app_ref, handle));

    assert!(
        !groups.is_empty(),
        "NIP-29 discovery stayed empty after direct relay probe proved kind:39000 data exists"
    );
    assert!(
        groups
            .iter()
            .all(|group| group.host_relay_url == NIP29_RELAY),
        "all discovered groups should be scoped to {NIP29_RELAY}: {groups:?}"
    );
}

fn relay_has_group_metadata() -> bool {
    let Ok(mut socket) = common::open_with_timeout(NIP29_RELAY, PROBE_BUDGET) else {
        return false;
    };
    if common::send_text(
        &mut socket,
        r#"["REQ","nmp-nip29-probe",{"kinds":[39000],"limit":1}]"#,
    )
    .is_err()
    {
        return false;
    }
    common::drain_until(&mut socket, Instant::now() + PROBE_BUDGET, |text| {
        text.starts_with(r#"["EVENT","nmp-nip29-probe""#)
    })
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
        match ticks.recv_timeout(remaining.min(Duration::from_secs(2))) {
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
