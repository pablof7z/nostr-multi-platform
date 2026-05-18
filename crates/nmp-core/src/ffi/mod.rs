//! Path-A raw C FFI surface. `mod.rs` carries the lifecycle + read-side
//! wrappers; `identity` carries the T66a identity / publish / multi-account
//! / relay-edit wrappers (split to keep each file under the 500-LOC cap).

mod identity;

use crate::actor::{run_actor, ActorCommand};
use crate::kernel::{is_hex_id, is_hex_pubkey};
use crate::relay::{DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
use std::ffi::{c_char, c_uint, c_void, CStr, CString};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

type UpdateCallback = extern "C" fn(*mut c_void, *const c_char);

#[derive(Clone, Copy)]
struct UpdateCallbackRegistration {
    context: usize,
    callback: UpdateCallback,
}

pub struct NmpApp {
    tx: Sender<ActorCommand>,
    update_callback: Arc<Mutex<Option<UpdateCallbackRegistration>>>,
    actor: Mutex<Option<JoinHandle<()>>>,
    update_listener: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for NmpApp {
    fn drop(&mut self) {
        if let Ok(mut callback) = self.update_callback.lock() {
            *callback = None;
        }
        let _ = self.tx.send(ActorCommand::Shutdown);
        if let Ok(mut actor) = self.actor.lock() {
            if let Some(handle) = actor.take() {
                let _ = handle.join();
            }
        }
        if let Ok(mut listener) = self.update_listener.lock() {
            if let Some(handle) = listener.take() {
                let _ = handle.join();
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn nmp_app_new() -> *mut NmpApp {
    let (command_tx, command_rx) = mpsc::channel();
    let (update_tx, update_rx) = mpsc::channel();
    let update_callback: Arc<Mutex<Option<UpdateCallbackRegistration>>> =
        Arc::new(Mutex::new(None));
    let listener_callback = Arc::clone(&update_callback);
    let actor = thread::spawn(move || run_actor(command_rx, update_tx));
    let update_listener = thread::spawn(move || {
        while let Ok(update) = update_rx.recv() {
            let Ok(payload) = CString::new(update) else {
                continue;
            };
            let callback = listener_callback.lock().ok().and_then(|guard| *guard);
            if let Some(registration) = callback {
                (registration.callback)(registration.context as *mut c_void, payload.as_ptr());
            }
        }
    });

    Box::into_raw(Box::new(NmpApp {
        tx: command_tx,
        update_callback,
        actor: Mutex::new(Some(actor)),
        update_listener: Mutex::new(Some(update_listener)),
    }))
}

// SAFETY: `app` is a raw pointer from `nmp_app_new()`. The function is `extern "C"` (callable
// from Swift/C) so it cannot be marked `unsafe` at the Rust level; the caller guarantees the
// pointer contract. The `allow` suppresses the clippy::not_unsafe_ptr_arg_deref lint which
// does not distinguish between `extern "C"` FFI boundaries and ordinary Rust functions.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_free(app: *mut NmpApp) {
    if !app.is_null() {
        // SAFETY: caller guarantees app is a valid pointer allocated by nmp_app_new().
        unsafe {
            drop(Box::from_raw(app));
        }
    }
}

#[no_mangle]
pub extern "C" fn nmp_app_set_update_callback(
    app: *mut NmpApp,
    context: *mut c_void,
    callback: Option<UpdateCallback>,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Ok(mut slot) = app.update_callback.lock() else {
        return;
    };
    *slot = callback.map(|callback| UpdateCallbackRegistration {
        context: context as usize,
        callback,
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_start(
    app: *mut NmpApp,
    _events_per_second: c_uint,
    visible_limit: c_uint,
    emit_hz: c_uint,
) {
    let Some(app) = app_ref(app) else {
        return;
    };

    let _ = app.tx.send(ActorCommand::Start {
        visible_limit: clamp_visible(visible_limit),
        emit_hz: clamp_emit_hz(emit_hz),
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_configure(
    app: *mut NmpApp,
    _events_per_second: c_uint,
    visible_limit: c_uint,
    emit_hz: c_uint,
) {
    let Some(app) = app_ref(app) else {
        return;
    };

    let _ = app.tx.send(ActorCommand::Configure {
        visible_limit: clamp_visible(visible_limit),
        emit_hz: clamp_emit_hz(emit_hz),
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_stop(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let _ = app.tx.send(ActorCommand::Stop);
}

#[no_mangle]
pub extern "C" fn nmp_app_reset(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let _ = app.tx.send(ActorCommand::Reset);
}

#[no_mangle]
pub extern "C" fn nmp_app_open_author(app: *mut NmpApp, pubkey: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }

    let _ = app.tx.send(ActorCommand::OpenAuthor { pubkey });
}

#[no_mangle]
pub extern "C" fn nmp_app_open_thread(app: *mut NmpApp, event_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(event_id) = c_string_argument(event_id) else {
        return;
    };
    if !is_hex_id(&event_id) {
        return;
    }

    let _ = app.tx.send(ActorCommand::OpenThread { event_id });
}

#[no_mangle]
pub extern "C" fn nmp_app_open_firehose_tag(app: *mut NmpApp, tag: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(tag) = c_string_argument(tag) else {
        return;
    };

    let _ = app.tx.send(ActorCommand::OpenFirehoseTag { tag });
}

#[no_mangle]
pub extern "C" fn nmp_app_claim_profile(
    app: *mut NmpApp,
    pubkey: *const c_char,
    consumer_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }

    let _ = app.tx.send(ActorCommand::ClaimProfile {
        pubkey,
        consumer_id,
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_release_profile(
    app: *mut NmpApp,
    pubkey: *const c_char,
    consumer_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }

    let _ = app.tx.send(ActorCommand::ReleaseProfile {
        pubkey,
        consumer_id,
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_close_author(app: *mut NmpApp, pubkey: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }

    let _ = app.tx.send(ActorCommand::CloseAuthor { pubkey });
}

#[no_mangle]
pub extern "C" fn nmp_app_close_thread(app: *mut NmpApp, event_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(event_id) = c_string_argument(event_id) else {
        return;
    };
    if !is_hex_id(&event_id) {
        return;
    }

    let _ = app.tx.send(ActorCommand::CloseThread { event_id });
}

/// Inject `count` pre-verified kind-1 events into the kernel timeline via
/// the test-support `ingest_pre_verified_event` path.
///
/// Events are constructed with deterministic IDs/pubkeys using
/// `VerifiedEvent::from_raw_unchecked` (test-support fast path; bypasses
/// Schnorr verification for harness ergonomics — see D0 note below).
///
/// D0: this symbol is gated on `cfg(any(test, feature = "test-support"))` and
/// is never part of the production FFI surface.  Swift/C callers never see it.
/// The `VerifiedEvent` type is the capability boundary: production code can
/// only construct one via `try_from_raw` (full Schnorr verify).  This function
/// uses `from_raw_unchecked` explicitly for legacy perf-harness compatibility.
///
/// Prefer `inject_signed_events` for new harnesses (S3/S4/S5 all use it now):
/// it produces real Schnorr-signed events via `EventBuilder::sign_with_keys`.
#[cfg(any(test, feature = "test-support"))]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_inject_pre_verified_events(
    app: *mut NmpApp,
    base_id_prefix: *const c_char,
    base_created_at: u64,
    count: u32,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let prefix = if base_id_prefix.is_null() {
        "stress".to_string()
    } else {
        // SAFETY: non-null pointer checked above.
        unsafe { CStr::from_ptr(base_id_prefix) }
            .to_str()
            .unwrap_or("stress")
            .to_string()
    };

    // Pool of 8 deterministic pubkeys (64 hex chars each) for the harness.
    const POOL: &[&str] = &[
        "0000000000000000000000000000000000000000000000000000000000000001",
        "0000000000000000000000000000000000000000000000000000000000000002",
        "0000000000000000000000000000000000000000000000000000000000000003",
        "0000000000000000000000000000000000000000000000000000000000000004",
        "0000000000000000000000000000000000000000000000000000000000000005",
        "0000000000000000000000000000000000000000000000000000000000000006",
        "0000000000000000000000000000000000000000000000000000000000000007",
        "0000000000000000000000000000000000000000000000000000000000000008",
    ];

    let events: Vec<crate::store::VerifiedEvent> = (0..count as u64)
        .map(|i| {
            // 64-hex event ID derived from prefix + index.
            let raw_id = format!("{prefix}{i:0>16x}");
            let id = format!("{raw_id:0<64}");
            let id = id[..64].to_string();
            let pubkey = POOL[(i as usize) % POOL.len()].to_string();
            let created_at = base_created_at.saturating_add(i);
            let content = format!("harness event {i}");
            let raw = crate::store::RawEvent {
                id,
                pubkey,
                created_at,
                kind: 1,
                tags: Vec::new(),
                content,
                // Placeholder sig — from_raw_unchecked bypasses verification.
                // D0 gate: this path is cfg-gated and excluded from the production
                // FFI ABI.  Use inject_signed_events for full Schnorr verify path.
                sig: "0".repeat(128),
            };
            crate::store::VerifiedEvent::from_raw_unchecked(raw)
        })
        .collect();

    let _ = app.tx.send(ActorCommand::IngestPreVerifiedEvents(events));
}

/// Inject `count` real Schnorr-signed kind-1 events into the kernel timeline
/// via the full `try_from_raw` verification path.
///
/// Uses `nostr::Keys::generate() + EventBuilder::text_note + sign_with_keys`
/// to produce cryptographically valid events.  Schnorr sign cost is ~30–50 µs
/// per event; for S4 (500 events) and S5 (200 events) this is 10–25 ms total.
///
/// D0: gated on `cfg(any(test, feature = "test-support"))`.  Not part of the
/// production FFI ABI.
#[cfg(any(test, feature = "test-support"))]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_inject_signed_events(
    app: *mut NmpApp,
    base_created_at: u64,
    count: u32,
) {
    use nostr::{EventBuilder, Keys, Timestamp};

    let Some(app) = app_ref(app) else {
        return;
    };

    // Single fixture key: generate once, sign all events.
    let keys = Keys::generate();
    let events: Vec<crate::store::VerifiedEvent> = (0..count as u64)
        .filter_map(|i| {
            let ts = Timestamp::from(base_created_at.saturating_add(i));
            let nostr_event = EventBuilder::text_note(format!("signed harness event {i}"))
                .custom_created_at(ts)
                .sign_with_keys(&keys)
                .ok()?;
            let raw = crate::store::RawEvent {
                id: nostr_event.id.to_hex(),
                pubkey: nostr_event.pubkey.to_hex(),
                created_at: nostr_event.created_at.as_secs(),
                kind: nostr_event.kind.as_u16() as u32,
                tags: nostr_event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                content: nostr_event.content.clone(),
                sig: nostr_event.sig.to_string(),
            };
            // try_from_raw: full Schnorr + id-hash verification.
            crate::store::VerifiedEvent::try_from_raw(raw).ok()
        })
        .collect();

    let _ = app.tx.send(ActorCommand::IngestPreVerifiedEvents(events));
}

pub(crate) fn app_ref<'a>(app: *mut NmpApp) -> Option<&'a NmpApp> {
    if app.is_null() {
        None
    } else {
        // SAFETY: caller guarantees non-null app is a valid NmpApp pointer.
        Some(unsafe { &*app })
    }
}

pub(crate) fn c_string_argument(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }

    // SAFETY: caller guarantees ptr is a valid null-terminated C string.
    // Validation: to_str() will reject invalid UTF-8.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

/// Optional-string FFI argument. Unlike `c_string_argument` (which collapses
/// NULL / empty / whitespace to `None` for a REQUIRED arg and the caller
/// drops the call), this returns `Some(value)` for non-empty content and
/// `None` for absent — so a NULL `reply_to_id` means "top-level note" rather
/// than "drop the publish". Build-doc §1.1 contract.
pub(crate) fn c_optional_string_argument(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees ptr is a valid null-terminated C string.
    let value = unsafe { CStr::from_ptr(ptr) }.to_str().ok()?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

fn clamp_visible(visible_limit: c_uint) -> usize {
    if visible_limit == 0 {
        DEFAULT_VISIBLE_LIMIT
    } else {
        visible_limit.clamp(1, 500) as usize
    }
}

fn clamp_emit_hz(emit_hz: c_uint) -> u32 {
    if emit_hz == 0 {
        DEFAULT_EMIT_HZ
    } else {
        emit_hz.clamp(1, 12)
    }
}
