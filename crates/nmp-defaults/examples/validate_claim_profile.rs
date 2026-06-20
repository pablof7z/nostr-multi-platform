//! Validate that `nmp_app_claim_profile` works without a logged-in user —
//! the kernel must auto-connect to the indexer relay (`purplepag.es`) and
//! fetch kind:0 for the claimed pubkey.
//!
//! Run (real network required — `purplepag.es` must be reachable):
//!
//!     cargo run --example validate_claim_profile -p nmp-defaults
//!
//! Exit code `0` on success (kind:0 surfaced into the snapshot inside the
//! 30-second window); `1` on timeout / shape mismatch.
//!
//! Why `nmp-defaults`, not `nmp-ffi`: without `register_defaults` the
//! kernel keeps `EmptyOutboxRouter`, every routing decision returns
//! `Unroutable`, and `claim_profile` is a no-op (no REQ ever reaches the
//! wire). This example needs the canonical composition.
//!
//! Observation surface: the typed `claimed_profiles` FlatBuffers sidecar
//! (PR-B #991/#979: the generic JSON payload no longer exists on the wire).
//! When a claimed profile's `display_name` becomes non-empty, the kind:0 fetch
//! succeeded and the display name is available.
//!
//! V-68 / V-112 (ADR-0042): `nmp_app_open_author` / `author_view` projection
//! deleted. This example was updated to observe via the typed `claimed_profiles`
//! sidecar instead.

use nmp_ffi::{
    nmp_app_claim_profile, nmp_app_free, nmp_app_new, nmp_app_set_update_callback, nmp_app_start,
};
use std::ffi::{c_void, CString};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

const PABLOF7Z_PUBKEY: &str = "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52";
const CONSUMER_ID: &str = "validate-test";
const TIMEOUT: Duration = Duration::from_secs(30);

extern "C" fn update_cb(context: *mut c_void, payload: *const u8, len: usize) {
    if context.is_null() || payload.is_null() {
        return;
    }
    // SAFETY: `payload` is borrowed for the callback lifetime; `context` is
    // the leaked `Box<Sender<Vec<u8>>>` from `main` and stays valid
    // program-wide. The raw frame bytes are copied out and decoded lazily on
    // the receiving side (typed sidecar — no JSON payload exists on the wire).
    let bytes = unsafe { std::slice::from_raw_parts(payload, len) };
    let tx = unsafe { &*(context as *const Sender<Vec<u8>>) };
    let _ = tx.send(bytes.to_vec());
}

/// Decode the typed `claimed_profiles` sidecar off a raw frame and return the
/// claimed profile's non-empty display name, if present for `pubkey`.
fn find_display_name(frame: &[u8], pubkey: &str) -> Option<String> {
    use nmp_core::typed_projections::{decode_claimed_profiles, CLAIMED_PROFILES_SCHEMA_ID};

    let typed = nmp_core::decode_snapshot_typed_projections(frame).ok()?;
    let entry = typed
        .iter()
        .find(|t| t.key == CLAIMED_PROFILES_SCHEMA_ID)
        .and_then(|t| decode_claimed_profiles(&t.payload).ok())?;
    entry
        .entries
        .iter()
        .find(|(pk, _)| pk == pubkey)
        .and_then(|(_, card)| card.display_name.as_ref())
        .filter(|s| !s.is_empty())
        .cloned()
}

fn dump_last_snapshot(frame: &[u8]) {
    use nmp_core::typed_projections::{decode_claimed_profiles, CLAIMED_PROFILES_SCHEMA_ID};

    if frame.is_empty() {
        eprintln!("  (no snapshot ticks observed)");
        return;
    }
    let Ok(envelope) = nmp_core::decode_snapshot_envelope(frame) else {
        eprintln!(
            "  (last frame was not a decodable snapshot; {} bytes)",
            frame.len()
        );
        return;
    };
    eprintln!(
        "  envelope: rev={}, events_rx={}, visible_items={}",
        envelope.rev, envelope.events_rx, envelope.visible_items
    );
    eprintln!("  relay_statuses = {:?}", envelope.relay_statuses);
    if let Some(view) = nmp_core::decode_snapshot_typed_projections(frame)
        .ok()
        .and_then(|typed| {
            typed
                .iter()
                .find(|t| t.key == CLAIMED_PROFILES_SCHEMA_ID)
                .and_then(|t| decode_claimed_profiles(&t.payload).ok())
        })
    {
        eprintln!("  typed claimed_profiles = {view:?}");
    }
}

fn main() -> std::process::ExitCode {
    let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel();
    let ctx = Box::into_raw(Box::new(tx)) as *mut c_void;

    let app = nmp_app_new();
    if app.is_null() {
        eprintln!("FAIL: nmp_app_new returned null");
        return std::process::ExitCode::from(1);
    }

    // SAFETY: `app` is a valid non-null pointer from `nmp_app_new`.
    nmp_defaults::register_defaults(unsafe { &mut *app });
    nmp_app_set_update_callback(app, ctx, Some(update_cb));
    nmp_app_start(app, 80, 4);

    let pubkey_c = CString::new(PABLOF7Z_PUBKEY).expect("pubkey has no NUL");
    let consumer_c = CString::new(CONSUMER_ID).expect("consumer has no NUL");
    println!("validate_claim_profile: claiming pubkey {PABLOF7Z_PUBKEY}");
    nmp_app_claim_profile(app, pubkey_c.as_ptr(), consumer_c.as_ptr(), 0, 0);

    let started = Instant::now();
    let mut ticks = 0usize;
    let mut last_payload: Vec<u8> = Vec::new();
    let mut exit_code = std::process::ExitCode::from(1);

    loop {
        let Some(remaining) = TIMEOUT
            .checked_sub(started.elapsed())
            .filter(|r| !r.is_zero())
        else {
            eprintln!(
                "FAIL: timed out after {:?} (ticks={ticks})",
                started.elapsed()
            );
            dump_last_snapshot(&last_payload);
            break;
        };
        match rx.recv_timeout(remaining) {
            Ok(payload) => {
                ticks += 1;
                last_payload = payload;
                if let Some(name) = find_display_name(&last_payload, PABLOF7Z_PUBKEY) {
                    println!(
                        "OK: received kind:0 in {:?} after {ticks} snapshot tick(s)",
                        started.elapsed()
                    );
                    println!("    surface       = claimed_profiles.display_name");
                    println!("    display_name  = {name:?}");
                    println!("    payload bytes = {}", last_payload.len());
                    exit_code = std::process::ExitCode::from(0);
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                eprintln!(
                    "FAIL: timed out after {:?} (ticks={ticks})",
                    started.elapsed()
                );
                dump_last_snapshot(&last_payload);
                break;
            }
            Err(RecvTimeoutError::Disconnected) => {
                eprintln!("FAIL: update channel disconnected (ticks={ticks})");
                break;
            }
        }
    }

    // `nmp_app_free` joins the actor + listener threads. The leaked
    // `Sender<Vec<u8>>` we passed as `context` is intentionally not
    // reclaimed — callbacks may still fire during shutdown drain.
    nmp_app_free(app);
    exit_code
}
