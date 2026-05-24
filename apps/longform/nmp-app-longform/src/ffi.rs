//! C-ABI surface for the long-form reader.
//!
//! Two entry points:
//!
//! * [`nmp_app_longform_init`] — initialise the singleton: build an `NmpApp`,
//!   register the projection + snapshot seam, add the host-supplied relays,
//!   start the actor, and push a kind:30023 tailing subscription.
//! * [`nmp_app_longform_snapshot_json`] — return the current article list as
//!   a JSON C string. The caller MUST free the returned string with
//!   `nmp_app_free_string` (re-exported from `nmp-core`); there is no
//!   bespoke freer here.
//!
//! The shape diverges from `nmp-app-fixture`'s `FfiApp` handle: the spike's
//! signatures take no `app` handle, so the kernel + store live in process-
//! global `OnceLock`s (same primitive `fixture-todo-core` uses for its
//! `TODO_STORE`).
//!
//! D0 — this file contains ZERO logic beyond C-string parsing + delegation
//! to substrate seams (`NmpApp::register_event_observer`,
//! `register_snapshot_projection`, `actor_sender`, `push_interest`,
//! `nmp_app_start`). The article-collection logic lives in `projection.rs`.

use std::collections::{BTreeSet, HashMap};
use std::ffi::{c_char, CStr, CString};
use std::sync::{Arc, Mutex, OnceLock};

use nmp_core::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use nmp_core::{ActorCommand, KernelEventObserver};
use nmp_ffi::{nmp_app_new, NmpApp};

use crate::projection::{
    ArticleStore, LongformProjection, ARTICLES_SNAPSHOT_KEY, KIND_LONGFORM,
};

/// Stable interest id for the kind:30023 tailing subscription. A fixed
/// constant means repeat `init` calls de-dupe in the kernel's interest
/// registry (idempotent re-push).
const LONGFORM_INTEREST_ID: InterestId = InterestId(0x10_4E_F0_4D_00_00_00_17);

/// Process-global app handle wrapper. `NmpApp` is held via raw pointer because
/// `nmp_app_new` returns `*mut NmpApp` and `nmp_app_free` consumes it; we leak
/// it for the process lifetime (the singleton mirrors the no-handle FFI
/// signature). The `unsafe impl Send + Sync` matches the posture of
/// `nmp-app-fixture::FfiApp` — the pointer is read-only after init, and the
/// host-init contract guarantees no aliasing `&NmpApp` exists during the
/// exclusive borrow inside `init`.
struct AppCell(*mut NmpApp);
// SAFETY: the `*mut NmpApp` is allocated once in `init`, never freed for the
// process lifetime, and after the exclusive borrow in `init` is dropped, only
// shared `&NmpApp` borrows are ever taken — identical posture to
// `nmp-app-fixture::FfiApp`'s `unsafe impl Send/Sync`.
unsafe impl Send for AppCell {}
unsafe impl Sync for AppCell {}

static APP: OnceLock<AppCell> = OnceLock::new();
static STORE: OnceLock<ArticleStore> = OnceLock::new();

/// Initialise the long-form reader: build the kernel, wire the projection,
/// add the host-supplied relays, start the actor, and subscribe to kind:30023.
///
/// `relays` is a C string holding a JSON array of relay URLs, e.g.
/// `["wss://relay.damus.io", "wss://nos.lol"]`. NULL or an empty array means
/// "no relays" — the kernel still starts (so the snapshot getter is callable)
/// but no events arrive. Malformed JSON is a silent no-op (D6 — failures are
/// data, never a panic at the FFI boundary).
///
/// Idempotent: the singleton is built on the first call; subsequent calls are
/// no-ops (the `OnceLock` ensures we never spawn a second actor). To rotate
/// relays after init, a future iteration would add a `set_relays` symbol.
///
/// # Safety
///
/// `relays` must be either NULL or a valid NUL-terminated C string with
/// valid UTF-8. Same convention as every other `nmp_app_*` symbol in
/// `nmp-core`.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_app_longform_init(relays: *const c_char) {
    // Build the kernel + store on the first call only. The closure runs at
    // most once per process; subsequent `init` calls take the existing handle.
    let store: ArticleStore = STORE
        .get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone();
    let app_ptr = APP.get_or_init(|| AppCell(build_and_wire(Arc::clone(&store)))).0;

    // Add each supplied relay through the public `ActorCommand::AddRelay`
    // door. `app.actor_sender()` is the documented Rust-side accessor for
    // pushing commands without the test-support FFI feature; the matching
    // C symbol `nmp_app_add_relay` is feature-gated. Role `read` is correct
    // for a read-only reader — write-side relay routing is not used.
    //
    // We re-apply the relay set on every `init` call (idempotent re-add inside
    // the actor) so a re-init with a new set converges to the latest list.
    if let Some(relay_urls) = parse_relays(relays) {
        // SAFETY: APP was just initialised above and is held for the process
        // lifetime; no concurrent mutation of the underlying `NmpApp` happens
        // after the `OnceLock` set — only shared `&NmpApp` reads, which the
        // pointer's posture (mirrored from `FfiApp`) permits.
        let app: &NmpApp = unsafe { &*app_ptr };
        let sender = app.actor_sender();
        for url in relay_urls {
            let _ = sender.send(ActorCommand::AddRelay {
                url,
                role: "read".to_string(),
            });
        }
    }
}

/// Return the current article list as a NUL-terminated JSON C string. The
/// shape is `{"articles":[{"id":"…","title":"…","author":"…","created_at":0},…]}`,
/// articles sorted by `created_at` descending (newest first).
///
/// Returns NULL if [`nmp_app_longform_init`] has not been called yet (no
/// store) or if JSON serialisation fails (impossible in practice — the
/// `Article` shape always serialises).
///
/// The returned pointer is heap-allocated; the caller MUST free it by passing
/// it to `nmp_app_free_string` (the same `nmp-core` symbol every other
/// `_json` getter in the substrate uses). No bespoke freer is introduced.
#[no_mangle]
pub extern "C" fn nmp_app_longform_snapshot_json() -> *const c_char {
    let Some(store) = STORE.get() else {
        return std::ptr::null();
    };
    let snapshot = LongformProjection::new(Arc::clone(store)).snapshot_json();
    let Ok(serialised) = serde_json::to_string(&snapshot) else {
        return std::ptr::null();
    };
    let Ok(cstr) = CString::new(serialised) else {
        return std::ptr::null();
    };
    cstr.into_raw().cast_const()
}

/// Internal: allocate an `NmpApp`, register the projection observer and the
/// snapshot projection, start the actor, and push the kind:30023 interest.
/// Returns the leaked raw pointer the singleton retains.
fn build_and_wire(store: ArticleStore) -> *mut NmpApp {
    let app_ptr = nmp_app_new();
    // SAFETY: `nmp_app_new` never returns null. The exclusive borrow here
    // mirrors `FfiApp::new`: no aliasing `&NmpApp` is live during init — we
    // only release the pointer into `APP` after this function returns.
    let app: &mut NmpApp = unsafe { &mut *app_ptr };

    // Observer side: a `KernelEventObserver` that mutates `store` on every
    // kind:30023 ingest. Cloned because the projection holds one `Arc` and
    // the snapshot closure below holds another — same pattern as
    // `nmp-app-chirp`'s `ZapsAggregateProjection`.
    let projection_for_observer = Arc::new(LongformProjection::new(Arc::clone(&store)));
    let observer_id = app.register_event_observer(
        Arc::clone(&projection_for_observer) as Arc<dyn KernelEventObserver>,
    );
    // D6 — a poisoned observer slot returns id 0; the host-extension thesis
    // says "fail silent at the seam", so we keep going (the snapshot getter
    // will still work, just always returning the empty list).
    let _ = observer_id;

    // Output side: register the host-extensible snapshot projection so the
    // articles surface in `KernelSnapshot::projections["longform.articles"]`
    // on every tick. A separate projection instance (sharing the same store)
    // so the closure can be `Send + Sync + 'static`.
    let projection_for_snapshot = LongformProjection::new(Arc::clone(&store));
    app.register_snapshot_projection(ARTICLES_SNAPSHOT_KEY, move || {
        projection_for_snapshot.snapshot_json()
    });

    // Start the actor. Pass 0 for every clamp so the kernel uses its defaults
    // — `nmp_app_start` treats 0 as "use default" (see `clamp_visible` and
    // `clamp_emit_hz` in `crates/nmp-core/src/ffi/mod.rs`).
    nmp_ffi::nmp_app_start(app_ptr, 0, 0, 0);

    // Push the kind:30023 tailing interest. Stable id so a repeat `init`
    // de-dupes in the kernel's interest registry. Global scope: long-form
    // discovery doesn't require an active account; the interest fans to the
    // host-supplied read relays added above.
    let mut kinds = BTreeSet::new();
    kinds.insert(KIND_LONGFORM);
    app.push_interest(LogicalInterest {
        id: LONGFORM_INTEREST_ID,
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds,
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    });

    app_ptr
}

/// Parse the `relays` C-string argument as a JSON array of strings. Returns
/// `None` on NULL, non-UTF-8, or non-array JSON — D6 absorbs every failure
/// shape into "no relays added".
fn parse_relays(relays: *const c_char) -> Option<Vec<String>> {
    if relays.is_null() {
        return None;
    }
    // SAFETY: caller guarantees `relays` is either NULL (returned above) or
    // a valid NUL-terminated C string. UTF-8 validation is below.
    let raw = unsafe { CStr::from_ptr(relays) }.to_str().ok()?;
    let parsed: Vec<String> = serde_json::from_str(raw).ok()?;
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_relays_handles_null() {
        assert!(parse_relays(std::ptr::null()).is_none());
    }

    #[test]
    fn parse_relays_decodes_string_array() {
        let json = CString::new(r#"["wss://a.relay","wss://b.relay"]"#).unwrap();
        let urls = parse_relays(json.as_ptr()).unwrap();
        assert_eq!(urls, vec!["wss://a.relay".to_string(), "wss://b.relay".to_string()]);
    }

    #[test]
    fn parse_relays_rejects_non_array_silently() {
        let json = CString::new(r#"{"not":"an array"}"#).unwrap();
        assert!(parse_relays(json.as_ptr()).is_none());
    }

    #[test]
    fn snapshot_returns_null_before_init() {
        // Calling the getter before `init` has populated the OnceLock must
        // not panic; it must return a NULL pointer the caller can guard on.
        // (This test only runs in isolation; if `init` was called by another
        // test in the same process the OnceLock is already populated, so we
        // skip the assertion in that case.)
        if STORE.get().is_none() {
            assert!(nmp_app_longform_snapshot_json().is_null());
        }
    }
}
