//! Raw signed-event tap FFI surface.
//!
//! Two `extern "C"` symbols exported to Swift / C, parallel to the
//! kernel-event observer trio in `ffi/event_observer.rs` but delivering the
//! VERBATIM inbound signed event (`sig` included), kind-filtered:
//!
//! - [`nmp_app_register_raw_event_observer`] — register a callback that
//!   fires once per accepted inbound event whose kind matches the supplied
//!   filter, with a nul-terminated JSON encoding of the flat NIP-01 signed
//!   event `{id, pubkey, created_at, kind, tags, content, sig}`. Returns a
//!   `u64` id for unregister.
//! - [`nmp_app_unregister_raw_event_observer`] — drop a registration by id.
//!
//! The Rust-side counterpart for in-process consumers is
//! [`crate::NmpApp::register_raw_event_observer`] /
//! `unregister_raw_event_observer` — both paths funnel into the same
//! `RawEventObserverSlot` the kernel taps from the single all-kinds ingest
//! point (`kernel/ingest/mod.rs::handle_event`) after the event passes the
//! kernel's existing Schnorr + id-hash gate.
//!
//! ## Wire contract (the inbound-ingest consumer depends on this verbatim)
//!
//! * **`kinds_json`** — a `*const c_char` holding a JSON array of u32 event
//!   kinds, e.g. `"[444,445,1059]"`. Null pointer, an empty array `"[]"`,
//!   or unparseable JSON all mean **deliver every kind** (no filter). The
//!   string is borrowed for the duration of the call; it is parsed into an
//!   owned filter and never retained.
//! * **callback payload** — a `*const c_char` nul-terminated JSON string =
//!   the verbatim flat NIP-01 signed event with fields, in order:
//!   `{"id","pubkey","created_at","kind","tags","content","sig"}`. `id` /
//!   `pubkey` / `sig` are lowercase hex; `created_at` is unix seconds;
//!   `kind` is a number; `tags` is an array of string arrays; `content` is
//!   a string. The `sig` is preserved byte-for-byte (the whole point).
//! * **C-string lifetime** — the payload pointer is borrowed for the
//!   duration of the callback only; consumers MUST copy any bytes they
//!   need. Same contract as `ffi/event_observer.rs` / `ffi/mod.rs`.
//!
//! ## Escape-hatch caveat
//!
//! Registering a raw-event tap **bypasses every guarantee the framework
//! normally provides**:
//!
//! * **D1** — the kernel's subscription/planner routing is invisible to your
//!   tap; you receive events regardless of whether any subscription asked for
//!   them, and after the kernel has already decided how to route and store them.
//! * **D3** — projection rules do not apply to tap payloads; you receive the
//!   raw wire event, not a projection-ready view object.
//! * **D5** — the tap runs outside the bounded snapshot cluster; high-volume
//!   kinds (e.g. kind:1 with a `null` filter) will fire the callback on every
//!   accepted inbound event with no back-pressure.
//! * **D8** — the tap callback is invoked on the raw-observer drain thread;
//!   any blocking operation in the callback stalls that drain, queuing events
//!   indefinitely.
//!
//! Use a raw tap only when you genuinely need the verbatim signed event
//! (`sig` field included) and kernel projections cannot supply it. The kernel
//! projection system (`NmpSnapshotProjector`) and action module seam
//! (`register_action::<M>`) are the doctrine-clean alternatives for the
//! overwhelming majority of use cases. See `docs/escape-hatches.md` for a
//! full catalogue of the four escape hatches and when each is appropriate.
//!
//! ## Doctrine
//!
//! * **D0** — generic capability. No protocol / NIP nouns in the symbol
//!   set; any consumer can tap raw inbound signed events. ADR-0009.
//! * **D6** — null app pointer, null callback, or poisoned mutex are
//!   silent no-ops; nothing crosses the FFI as an exception.

use super::{app_ref, NmpApp};
use crate::actor::{
    register_c_raw_observer, unregister_raw_observer, KindFilter, RawEventObserverFn,
    RawEventObserverId, RawEventObserverRegistration,
};
use std::ffi::{c_char, c_void, CStr};

/// Parse a borrowed `kinds_json` C pointer into a [`KindFilter`].
///
/// Null pointer, empty array, or any parse failure → match-everything
/// filter (D6: a malformed filter must not silently drop all events; the
/// safe degraded behaviour is "deliver all", which the consumer can still
/// filter client-side).
fn parse_kind_filter(kinds_json: *const c_char) -> KindFilter {
    if kinds_json.is_null() {
        return KindFilter::default();
    }
    // SAFETY: FFI contract — borrowed nul-terminated C string for the
    // duration of this call.
    let cstr = unsafe { CStr::from_ptr(kinds_json) };
    let Ok(s) = cstr.to_str() else {
        return KindFilter::default();
    };
    match serde_json::from_str::<Vec<u32>>(s) {
        Ok(kinds) => KindFilter::from_kinds(kinds),
        Err(_) => KindFilter::default(),
    }
}

/// Register a C-ABI raw signed-event observer.
///
/// `callback` fires on the raw-observer drain thread once per inbound
/// event that has passed the kernel's Schnorr + id-hash gate, entered
/// canonical store semantics, AND whose `kind` matches `kinds_json`.
/// The C string argument is a nul-terminated JSON encoding
/// of the verbatim flat NIP-01 signed event
/// `{id, pubkey, created_at, kind, tags, content, sig}`.
///
/// `kinds_json` is a JSON array of u32 kinds (e.g. `"[445,1059]"`); a null
/// pointer, `"[]"`, or unparseable input means "deliver every kind".
///
/// Returns a non-zero `u64` id on success; `0` on failure (null app, null
/// callback, or poisoned mutex). The id is required to unregister.
#[no_mangle]
pub extern "C" fn nmp_app_register_raw_event_observer(
    app: *mut NmpApp,
    context: *mut c_void,
    callback: Option<RawEventObserverFn>,
    kinds_json: *const c_char,
) -> u64 {
    let Some(app) = app_ref(app) else {
        return 0;
    };
    let Some(callback) = callback else {
        return 0;
    };
    let kinds = parse_kind_filter(kinds_json);
    let registration = RawEventObserverRegistration {
        context: context as usize,
        callback,
        kinds,
    };
    let slot = app.raw_event_observers_slot();
    let id = register_c_raw_observer(&slot, registration);
    id.0
}

/// Drop a previously-registered raw observer by id. Idempotent: unknown
/// ids, null app pointers, or poisoned mutexes are silent no-ops (D6).
#[no_mangle]
pub extern "C" fn nmp_app_unregister_raw_event_observer(app: *mut NmpApp, id: u64) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let slot = app.raw_event_observers_slot();
    unregister_raw_observer(&slot, RawEventObserverId(id));
}

#[cfg(test)]
mod tests {
    //! End-to-end: register a C raw tap with a kind filter, push a REAL
    //! Schnorr-signed event through the kernel's `handle_event` ingest
    //! path, assert the callback receives byte-faithful JSON including a
    //! valid `sig`, and that a non-matching kind is filtered out.

    use super::*;
    use crate::ffi::{nmp_app_free, nmp_app_new};
    use std::ffi::CString;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Mutex, OnceLock};

    // Plain `extern "C" fn` can't capture; park a `Sender` in a static and
    // forward the payload string through it. SERIAL linearises tests.
    static EVENTS_TX: OnceLock<Mutex<Option<Sender<String>>>> = OnceLock::new();
    static SERIAL: Mutex<()> = Mutex::new(());

    extern "C" fn record_callback(_ctx: *mut c_void, payload: *const c_char) {
        if payload.is_null() {
            return;
        }
        // SAFETY: callback contract — payload is a valid nul-terminated C
        // string borrowed for the duration of the call.
        let cstr = unsafe { CStr::from_ptr(payload) };
        if let Ok(s) = cstr.to_str() {
            if let Some(slot) = EVENTS_TX.get() {
                if let Ok(guard) = slot.lock() {
                    if let Some(tx) = guard.as_ref() {
                        let _ = tx.send(s.to_string());
                    }
                }
            }
        }
    }

    fn install_recorder() -> std::sync::mpsc::Receiver<String> {
        let (tx, rx) = channel::<String>();
        let slot = EVENTS_TX.get_or_init(|| Mutex::new(None));
        *slot.lock().unwrap() = Some(tx);
        rx
    }

    fn uninstall_recorder() {
        if let Some(slot) = EVENTS_TX.get() {
            *slot.lock().unwrap() = None;
        }
    }

    #[test]
    fn register_and_unregister_round_trip() {
        let _g = SERIAL.lock().unwrap();
        let _rx = install_recorder();
        let app = nmp_app_new();
        let kinds = CString::new("[1]").unwrap();
        let id = nmp_app_register_raw_event_observer(
            app,
            std::ptr::null_mut(),
            Some(record_callback),
            kinds.as_ptr(),
        );
        assert!(id > 0, "register returned zero id");
        nmp_app_unregister_raw_event_observer(app, id);
        nmp_app_free(app);
        uninstall_recorder();
    }

    #[test]
    fn null_callback_returns_zero_id() {
        let _g = SERIAL.lock().unwrap();
        let app = nmp_app_new();
        let id =
            nmp_app_register_raw_event_observer(app, std::ptr::null_mut(), None, std::ptr::null());
        assert_eq!(id, 0);
        nmp_app_free(app);
    }

    #[test]
    fn null_app_is_silent() {
        let _g = SERIAL.lock().unwrap();
        let id = nmp_app_register_raw_event_observer(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            Some(record_callback),
            std::ptr::null(),
        );
        assert_eq!(id, 0);
        nmp_app_unregister_raw_event_observer(std::ptr::null_mut(), 1);
    }

    #[test]
    fn null_kinds_json_means_all_kinds() {
        // A null filter must register successfully (match everything),
        // never reject the registration.
        let filter = parse_kind_filter(std::ptr::null());
        assert!(filter.is_all());
        let bad = CString::new("not json").unwrap();
        assert!(parse_kind_filter(bad.as_ptr()).is_all());
        let empty = CString::new("[]").unwrap();
        assert!(parse_kind_filter(empty.as_ptr()).is_all());
        let some = CString::new("[445,1059]").unwrap();
        let f = parse_kind_filter(some.as_ptr());
        assert!(!f.is_all());
        assert!(f.matches(445) && f.matches(1059) && !f.matches(1));
    }
}
