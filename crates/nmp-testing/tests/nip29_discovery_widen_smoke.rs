//! Hermetic public-runtime smoke for NIP-29 discovery **widening** (chirp#93
//! reconcile primitive; chirp#156/S73 regression).
//!
//! chirp#156 reports that adding a relay dynamically (Chirp's "Add a relay"
//! affordance) never surfaces that relay's groups on a STRICT NIP-29 relay:
//! the relay's own query-validation policy denies the discovery subscription
//! with "must have 'h', 'e' or 'a' tag" — but only for the newly-added relay,
//! never for a relay present in the initial (curated-defaults) set. This test
//! proves (or disproves) the suspected root cause at the NMP layer: that the
//! reconcile-appended relay's compiled REQ filter diverges from the
//! initial-set relay's REQ filter. Two independent recording relays stand in
//! for "a curated default" (A, live from app boot) and "a dynamically added
//! relay" (B, never named at boot — added only via a second, reconciling
//! `open_nip29_group_discovery_session_with_reader` call, exactly mirroring
//! `DiscoveredGroupsStore.addRelay`/`beginDiscovery`).

#[path = "common/mod.rs"]
mod common;

use std::ffi::c_void;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};

use common::recording_relay::RecordingRelay;
use nmp_native_runtime::{NmpApp, NmpAppBuilder, RunConfig};
use nmp_nip29::{
    close_nip29_group_discovery_session, open_nip29_group_discovery_session_with_reader,
    Nip29GroupDiscoverySession,
};

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
    #[allow(dead_code)]
    ticks: Receiver<()>,
}

impl DiscoveryApp {
    /// Boots with ONE relay in the initial relay list — mirroring Chirp's
    /// curated-defaults set at app start. `relay_b` is deliberately NOT
    /// named here: it must reach the wire only through the later reconciling
    /// `open_nip29_group_discovery_session_with_reader` call, the same way a
    /// relay the user types into "Add a relay" was never in Chirp's boot
    /// relay list either.
    fn boot(relay_a: &str) -> Self {
        let mut builder = NmpAppBuilder::new();
        nmp_substrate::install(&mut builder, nmp_substrate::SubstrateConfig::default());
        nmp_nip29::register(&mut builder, nmp_nip29::Config::default())
            .expect("nmp-nip29 registration must not collide");

        let app = builder
            .in_memory()
            .consume_all_builtin_projections()
            .with_relays([(relay_a, "both")])
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

/// THE WIDEN-SYMMETRY PROOF (chirp#156/S73): reconciling a live NIP-29
/// group-discovery session onto a SUPERSET of relays (adding relay B to an
/// already-open session for relay A — exactly `DiscoveredGroupsStore.addRelay`'s
/// shape) must compile the SAME filter shape for B as A got at initial open:
/// kinds {39000,39001,39002}, no `#d`/`#h`/`#e`/`#a` tag key at all. A strict
/// NIP-29 relay's "must have 'h', 'e' or 'a' tag" denial for a dynamically
/// added relay (and NOT for the initial set) would only make sense if this
/// test failed — i.e. if B's compiled REQ carried a tag filter A's didn't.
#[test]
fn widen_reconcile_compiles_identical_filter_for_appended_relay() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());

    let mut relay_a = RecordingRelay::spawn(Vec::new());
    let mut relay_b = RecordingRelay::spawn(Vec::new());
    let app = DiscoveryApp::boot(relay_a.url());
    let app_ref = unsafe { &*app.app };

    // Initial open: relay A only (the "curated defaults" shape).
    let (handle_a, _reader) = open_nip29_group_discovery_session_with_reader(
        app_ref,
        Nip29GroupDiscoverySession::new(vec![relay_a.url().to_string()]),
    );
    let req_a = relay_a.wait_req("initial-set discovery REQ (relay A)", |_| true);

    // Reconcile-widen: relay B joins the desired set — relay B was never
    // named at boot, mirroring a brand-new "Add a relay" entry. Reuse the
    // SAME projection key (the function reconciles by key, not by handle).
    let (handle_b, _reader) = open_nip29_group_discovery_session_with_reader(
        app_ref,
        Nip29GroupDiscoverySession::new(vec![relay_a.url().to_string(), relay_b.url().to_string()]),
    );
    let req_b = relay_b.wait_req("reconcile-appended discovery REQ (relay B)", |_| true);
    eprintln!("relay A REQ filter: {}", req_a.filter);
    eprintln!("relay B REQ filter: {}", req_b.filter);

    let shape_a = filter_shape(&req_a.filter);
    let shape_b = filter_shape(&req_b.filter);
    assert_eq!(
        shape_a, shape_b,
        "relay B's (reconcile-appended) compiled REQ filter must be byte-identical in shape \
         to relay A's (initial-set) REQ filter — a divergence here is exactly the chirp#156/S73 \
         root cause: a strict NIP-29 relay denies a query missing the metadata-kind restriction \
         with \"must have 'h', 'e' or 'a' tag\", so any extra/different tag key on B's REQ would \
         explain why only dynamically-added relays are denied"
    );
    assert!(
        shape_a.no_tag_keys,
        "the discovery filter must never carry a tag key ('#d'/'#h'/'#e'/'#a') — kind:39000-39002 \
         metadata queries need none, and a strict relay treats an unscoped, mistagged query as \
         invalid rather than as \"all metadata\""
    );

    // Both handles address the SAME singleton discovery session (framework
    // doctrine: one session, one projection key) — closing it once via the
    // most recent handle is the documented contract (mirrors
    // `nmp_app_chirp_open_group_discovery`'s doc comment: "only the FINAL
    // handle for a browsing session is closed").
    let _ = handle_a;
    assert!(close_nip29_group_discovery_session(app_ref, handle_b));
}

/// The parts of a REQ filter that matter for this proof: its kind set and
/// whether it carries ANY tag-filter key (`#`-prefixed). Deliberately
/// ignores field ORDER (`serde_json::Value` equality already ignores key
/// order for objects) — only the semantic shape is asserted.
#[derive(Debug, PartialEq, Eq)]
struct FilterShape {
    kinds: Vec<u64>,
    no_tag_keys: bool,
}

fn filter_shape(filter: &serde_json::Value) -> FilterShape {
    let mut kinds: Vec<u64> = filter
        .get("kinds")
        .and_then(|k| k.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default();
    kinds.sort_unstable();
    let no_tag_keys = filter
        .as_object()
        .map(|obj| obj.keys().all(|k| !k.starts_with('#')))
        .unwrap_or(true);
    FilterShape { kinds, no_tag_keys }
}
