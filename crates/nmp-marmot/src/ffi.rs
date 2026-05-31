//! Marmot (MLS-over-Nostr) per-app FFI surface.
//!
//! Five `extern "C"` symbols Swift links against — they mirror the
//! lifetime / free / D6 conventions of the Chirp timeline symbols
//! (`nmp_app_chirp_register` / `_snapshot` / `_snapshot_free` /
//! `_unregister`):
//!
//! - [`nmp_marmot_register`] — build a [`MarmotService`]
//!   (signer seam: secret key hex/nsec passed directly; DB at
//!   `<app_support>/marmot-mls-state.sqlite`), register the lossy
//!   `KernelEvent` metadata observer AND the raw signed-event inbound
//!   tap (kinds `[444, 445, 1059]`), return an opaque `*mut MarmotHandle`.
//! - [`nmp_marmot_snapshot`] — JSON snapshot
//!   (`groups` / `pending_welcomes` / `key_package`).
//! - [`nmp_marmot_group_messages`] — newest-N decrypted messages
//!   for one group (hex id), JSON array.
//! - [`nmp_marmot_string_free`] — companion deallocator.
//! - [`nmp_marmot_unregister`] — drop both kernel
//!   registrations (lossy observer + raw tap) + free the handle.
//!   Idempotent.
//!
//! ## Mutating ops — `nmp_app_dispatch_action` + Rust-native accessor
//!
//! The legacy bespoke `nmp_marmot_dispatch` C-ABI symbol was deleted in
//! ADR-0025 PR 3 (2026-05-23). Mutating ops now have two entry points:
//!
//! * **Host (iOS)** — `nmp_app_dispatch_action("nmp.marmot", action_json)`,
//!   the generic kernel dispatch path. Registered in
//!   [`register_with_keys`] via
//!   [`crate::projection::action::MarmotActionModule`] +
//!   [`crate::projection::handler::MarmotMlsOpHandler`]. Returns a
//!   `correlation_id` synchronously; the terminal verdict is mirrored on
//!   the `action_stages` projection. The rich per-op envelope is consumed
//!   by the kernel, not surfaced to the host.
//! * **In-process Rust callers (REPL / TUI / integration tests)** —
//!   [`MarmotHandle::dispatch`], a Rust-native method that reaches the
//!   SAME [`crate::projection::ops::dispatch`] entry point both seams use
//!   and returns the FULL synchronous envelope (`events`,
//!   `welcome_rumors`, `evolution_event`, `event`,
//!   `post_join_self_update_event`, …). Required by the hand-shuttle MLS
//!   round-trip in `crates/chirp-repl/src/marmot.rs::tests`.
//!
//! ## Doctrine
//!
//! * **D0** — `nmp-core` never depends on `nmp-marmot`; this crate is the
//!   composition point (ADR-0009, kernel boundary). No MLS / MDK type
//!   crosses this FFI — `group_id` is hex, errors are strings, exactly the
//!   typed translation layer `nmp-marmot` asked a consumer to provide.
//! * **D6** — every entry point is fire-and-forget. Null pointers, missing
//!   strings, JSON parse / serialize failures, poisoned mutexes, and
//!   `MarmotService` errors all degrade to `null` / `{"ok":false}` rather
//!   than panicking across the FFI.
//!
//! ## Outbound relay seam — CLOSED
//!
//! Where an op produces events that must reach relays
//! (`publish_key_package`'s kind:30443/443, `create_group` /
//! `invite`'s kind:445 commit + kind:1059 gift-wraps, `send`'s kind:445,
//! `accept_welcome`'s post-join kind:445 self-update), this crate performs
//! the `MarmotService` op and then publishes the signed events INTERNALLY
//! via [`crate::projection::publish`] (the workspace-internal
//! `nmp_ffi::NmpApp::publish_signed_explicit` kernel API, called against
//! the retained `&NmpApp`). There is NO Swift relay path — that hook never
//! existed (see `MarmotBridge.swift`). The result still carries the signed
//! event JSON (`event` / `events` / `evolution_event` / `welcome_rumors`)
//! but it is now INFORMATIONAL only; publish already happened
//! (fire-and-forget — success == "submitted to the kernel publish
//! pipeline"). Routing per kind: kind:445 → group-pinned relays
//! (`Explicit`, cache miss → `Auto`); kind:30443/443 → author outbox
//! (`Auto`); kind:1059 gift-wrap → group relays as a documented
//! inbox-routing approximation. The MDK pending-commit is still resolved
//! here (commit eagerly because the events are produced + submitted;
//! clear-on-failure is exposed via the `clear_pending` op).
//!
//! ## Inbound ingest seam — CLOSED
//!
//! `nmp_marmot_register` also registers a raw signed-event tap
//! (`nmp-core` `RawEventObserver`, Rust-trait API) for kinds
//! `[444, 445, 1059]`. The kernel delivers every accepted inbound signed
//! event of those kinds to [`crate::projection::tap`], which drives them
//! through the SAME `ops::ingest_signed_event_core` the back-compat
//! `{"op":"ingest_signed_event"}` dispatch op uses — so welcomes /
//! messages received from relays surface in the next snapshot with no
//! Swift involvement (the existing snapshot read is unchanged).
//! `nmp_marmot_unregister` tears down BOTH kernel
//! registrations (the lossy `KernelEvent` metadata observer AND the raw
//! tap; distinct slots / ids). This was the last open seam.

use std::ffi::{c_char, CStr, CString};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use nmp_core::{KernelEventObserverId, RawEventObserver, RawEventObserverId};
use nmp_ffi::NmpApp;
use nostr::Keys;
use serde_json::{json, Value};

use crate::service::MarmotService;

use crate::projection::action::MarmotActionModule;
use crate::projection::handler::MarmotMlsOpHandler;
use crate::projection::state::MarmotProjection;
use crate::projection::tap::MarmotIngestTap;

/// Default page size for [`nmp_marmot_group_messages`].
const DEFAULT_MESSAGE_PAGE: usize = 200;

/// Keyring coordinates for the production encrypted SQLite DB. Stable
/// strings — the keyring entry is created lazily by `MdkSqliteStorage`.
const KEYRING_SERVICE_ID: &str = "nmp.chirp.marmot";
const KEYRING_DB_KEY_ID: &str = "marmot-mls-db-key";

/// Opaque handle returned by [`nmp_marmot_register`]. Boxed so the
/// address is stable; Swift holds the raw pointer until
/// [`nmp_marmot_unregister`].
pub struct MarmotHandle {
    projection: Arc<MarmotProjection>,
    /// Lossy `KernelEvent` observer (key-package metadata tracker — see
    /// `MarmotProjection::on_kernel_event`). Distinct slot / id from the
    /// raw tap below; both are torn down in `unregister`.
    observer_id: KernelEventObserverId,
    /// Raw signed-event tap (the CLOSED inbound ingest seam — drives
    /// kind:1059/445 into `MarmotService` via the shared core; see
    /// [`crate::projection::tap`]). Separate kernel slot from `observer_id`.
    raw_observer_id: RawEventObserverId,
    pub(crate) app: *mut NmpApp,
}

// SAFETY: identical rationale to `ChirpHandle` (see `crate::ffi`). The
// auto-derived `!Send`/`!Sync` comes only from `app: *mut NmpApp`; the
// `Arc<MarmotProjection>` is already `Send + Sync`. The earlier comment's
// claim that "Swift drives every call from one serialized bridge dispatch
// queue" is NOT accurate — `KernelHandle` is a plain `final class` with no
// queue. The honest invariant has three layers:
//
//   1. Swift owns this handle and only reaches the FFI entry points below
//      from `@MainActor` types (`KernelModel` / `MarmotStore`), so the
//      handle struct itself is never raced (a documented Swift caller
//      convention, not a type guarantee).
//   2. The `Arc<MarmotProjection>` IS shared across threads — the kernel
//      actor thread runs `MarmotProjection::on_kernel_event` and the raw
//      tap's `on_raw_event` while the Swift main actor calls `snapshot()` /
//      dispatch. Soundness of that sharing comes from `MarmotProjection`'s
//      interior `Mutex<Inner>`, not from this `unsafe impl`.
//   3. The `app` raw pointer is only read (to forward fire-and-forget
//      kernel commands). No use-after-free is possible: `nmp_app_free`'s
//      `NmpApp::Drop` sends `Shutdown` and `join()`s the actor thread
//      before freeing the allocation, and every kernel callback that can
//      reach `app` (`on_kernel_event`, `on_raw_event`) runs INLINE on that
//      actor thread — the join fences them.
//
// CALLER CONTRACT: `nmp_app_free` must not run while a kernel callback that
// reaches this projection is still executing. The in-process Rust-trait
// registration path used here (`register_event_observer` /
// `register_raw_event_observer`) gets that fence from the actor join.
// Calling `nmp_marmot_unregister` before `nmp_app_free` is the
// documented hygiene step; the actor join is the actual fence.
unsafe impl Send for MarmotHandle {}
unsafe impl Sync for MarmotHandle {}

impl MarmotHandle {
    /// Rust-native snapshot accessor for in-process callers (REPL / TUI /
    /// integration tests). Returns the same [`crate::projection::payload::MarmotSnapshot`]
    /// the push projection emits under `"nmp.marmot.snapshot"` on the
    /// SnapshotFrame, without any C-ABI round-trip.
    ///
    /// Prefer this over the deprecated [`nmp_marmot_snapshot`] C-ABI symbol in
    /// Rust code. Swift consumers read from the pushed SnapshotFrame projection
    /// key instead.
    #[must_use]
    pub fn snapshot_rust(&self) -> crate::projection::payload::MarmotSnapshot {
        self.projection.snapshot(now_secs())
    }

    /// Rust-native messages accessor for in-process callers (REPL / TUI /
    /// integration tests). Returns the newest-N decrypted messages for
    /// `group_id_hex`, using the same [`crate::projection::ops::group_messages`]
    /// path the push projection (`"nmp.marmot.messages"`) uses.
    ///
    /// Returns an empty `Vec` on any soft failure (unknown group, poisoned
    /// mutex — D6 non-panicking degradation).
    ///
    /// Prefer this over the deprecated [`nmp_marmot_group_messages`] C-ABI
    /// symbol in Rust code. Swift consumers read from the pushed SnapshotFrame
    /// projection key instead.
    #[must_use]
    pub fn messages_rust(
        &self,
        group_id_hex: &str,
    ) -> Vec<crate::projection::payload::MarmotMessageRow> {
        self.projection
            .with_inner(|h| {
                crate::projection::ops::group_messages(h, group_id_hex, DEFAULT_MESSAGE_PAGE)
            })
            .unwrap_or_default()
    }

    /// Rust-native dispatch entry point for in-process callers (REPL / TUI /
    /// integration tests) that need the SYNCHRONOUS rich per-op envelope —
    /// `events` for `publish_key_package`, `welcome_rumors` /
    /// `evolution_event` / `group_id_hex` for `create_group` / `invite`,
    /// `event` for `send`, `post_join_self_update_event` for
    /// `accept_welcome`, etc.
    ///
    /// ## Why this exists separately from `nmp_app_dispatch_action`
    ///
    /// ADR-0025 PR 3 deleted the legacy bespoke `nmp_marmot_dispatch` C-ABI
    /// symbol; iOS now routes every Marmot op through the generic
    /// `nmp_app_dispatch_action("nmp.marmot", action_json)` path
    /// ([`crate::projection::action::MarmotActionModule`]). That path is
    /// non-blocking — it returns `{"correlation_id":"…"}` synchronously and
    /// the rich envelope produced by the `MarmotMlsOpHandler` is consumed
    /// by the kernel's `action_stages` machinery (which only mirrors the
    /// `ok:true/false` verdict). The per-op event payloads are NOT surfaced
    /// to the caller on that path.
    ///
    /// In-process Rust callers that hand-shuttle MLS events between
    /// `AppRuntime`s — namely `chirp-repl` / `chirp-tui` / their
    /// integration tests — depend on the synchronous envelope. This
    /// accessor invokes the SAME [`crate::projection::ops::dispatch`]
    /// entry point both seams reach (the kernel actor's `DispatchHostOp`
    /// arm and the legacy C symbol used) without going through any FFI.
    ///
    /// ## D0 / layering
    ///
    /// This is a Rust-native method on a `pub` opaque handle in this app
    /// crate. It is NOT a C-ABI symbol, not part of any host FFI surface,
    /// and not subject to ADR-0025's bespoke-FFI prohibition (which
    /// targeted `extern "C"` cluster bloat in the iOS bridge).
    pub fn dispatch(&self, action: &Value) -> Value {
        self.projection
            .with_inner(|h| crate::projection::ops::dispatch(h, action, now_secs()))
            .unwrap_or_else(|| json!({
                "ok": false,
                "error": "projection mutex poisoned",
            }))
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn publish_key_package_on_register(handle: *mut MarmotHandle) {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return;
    };
    let action = json!({ "op": "publish_key_package" });
    let _ = handle
        .projection
        .with_inner(|h| crate::projection::ops::dispatch(h, &action, now_secs()));
}

/// Inner registration logic shared by `nmp_marmot_register` and
/// `nmp_marmot_register_active`. `app` must be non-null and valid.
///
/// ## Keyring policy (V-62)
///
/// `credential_store::initialize()` installs the platform keyring store once.
/// On Apple platforms it tries the real Keychain first; if that fails it
/// switches to the in-memory mock store (returns `Some(true)` = mock).
/// On non-Apple platforms (Linux, WASM) the mock store is always used
/// (returns `Some(true)`).
///
/// **Critical constraint**: when `initialize()` returns `Some(false)` (real
/// Apple Keychain was configured), a subsequent `MarmotService::new` failure
/// must NOT silently fall through to the mock store. That path was the V-62
/// violation: MLS secrets would live only in memory with no host signal,
/// making every group unjoinable on the next launch.
///
/// The corrected policy:
/// - If `initialize()` chose the real Keychain (`use_mock = false`) and
///   `MarmotService::new` fails, return null. The host observes the null
///   handle and may surface a recovery prompt or retry.
/// - If `initialize()` already chose the mock store (`use_mock = true`)
///   the service init failing is also fatal (return null).
/// - The mock store is ONLY legitimately installed when `initialize()` chose
///   it (non-Apple platform or Apple platform with no Keychain entitlement).
///   In that case `keyring_unavailable = true` is set on the projection so
///   the snapshot surfaces the diagnostic to the host.
pub(crate) fn register_with_keys(app: *mut NmpApp, keys: Keys, db_path: &str) -> *mut MarmotHandle {
    let Some(use_mock) = crate::credential_store::initialize() else {
        return std::ptr::null_mut();
    };

    // V-62: `use_mock` is `true` only when `initialize()` explicitly chose
    // the in-memory mock store (non-Apple platform, or Apple platform where
    // no Keychain entitlement is available). We surface this as
    // `keyring_unavailable = true` in the projection snapshot so the host can
    // warn the user. We never silently switch from the real Keychain to mock.
    let service = match MarmotService::new(db_path, KEYRING_SERVICE_ID, KEYRING_DB_KEY_ID, keys.clone()) {
        Ok(s) => s,
        Err(e) => {
            // Both the real-keyring path and the already-mock path are hard
            // failures here. The old code silently installed the mock store
            // when the real keyring failed (`!use_mock` branch) — that was
            // the V-62 silent durability loss. We now return null on all
            // service-init failures so the host observes the error.
            eprintln!(
                "nmp-marmot: keyring/service init failed (use_mock={use_mock}): {e}; \
                 returning null handle — host must surface MarmotInitError::KeyringUnavailable"
            );
            return std::ptr::null_mut();
        }
    };

    // Step 1: register the substrate-generic `MarmotActionModule` against
    // the kernel's action registry. This is the SOLE host entry point
    // for Marmot mutating ops (the legacy bespoke `nmp_marmot_dispatch`
    // C-ABI symbol was deleted in ADR-0025 PR 3, 2026-05-23); hosts
    // reach every Marmot write through
    // `nmp_app_dispatch_action("nmp.marmot", action_json)`. Registration
    // is idempotent (replaces any prior entry under the same namespace),
    // so a second `register_with_keys` (account switch) is safe. Takes
    // `&mut NmpApp` and must run BEFORE any other `&NmpApp` borrow below.
    //
    // SAFETY: the caller guarantees `app` is a valid pointer from
    // `nmp_app_new`. No other reference aliases `app` at this point — the
    // `&*app` borrow on the next line is taken only after this exclusive
    // borrow is dropped. Mirrors the `register_chirp_actions(unsafe { &mut
    // *app })` pattern in `apps/chirp/nmp-app-chirp/src/ffi/register.rs`.
    unsafe { &mut *app }.register_action::<MarmotActionModule>();

    // SAFETY: caller guarantees `app` is non-null and valid.
    let app_ref = unsafe { &*app };
    // V-62: pass `use_mock` as `keyring_unavailable` so the projection
    // surfaces the diagnostic in every snapshot. The host reads
    // `snapshot.keyring_unavailable` and may block group features or prompt
    // the user to resolve the Keychain issue.
    let projection = Arc::new(MarmotProjection::new(service, use_mock));
    projection.set_app(app);

    // V-107 / ADR-0039: register the two Marmot push projections onto the
    // canonical snapshot seam. Both ride the SnapshotFrame on every tick
    // that `changed_since_emit` is set — no polling (D8). The kernel marks
    // `changed_since_emit` on every accepted inbound relay event (including
    // kind:445/1059 handled by the raw ingest tap), so new messages / welcomes
    // surface in the next pushed frame edge-triggered.
    //
    // `register_snapshot_projection` is lock-and-push; calling it here
    // (post-construction, before `nmp_app_start`) is the documented pattern.
    // A second `register_with_keys` (account switch) replaces the prior closure
    // under the same key (the registry `register` method is replace-by-key).
    //
    // **`nmp.marmot.snapshot`** — replaces the pull-symbol
    // `nmp_marmot_snapshot`: group list / membership / key-package / pending
    // welcomes. Cheap: one lock + MDK SQLite reads, no re-decrypt.
    {
        let snap_projection = Arc::clone(&projection);
        app_ref.register_snapshot_projection("nmp.marmot.snapshot", move || {
            serde_json::to_value(snap_projection.snapshot(now_secs()))
                .unwrap_or(serde_json::Value::Null)
        });
    }

    // **`nmp.marmot.messages`** — replaces the parameterized pull-symbol
    // `nmp_marmot_group_messages(group_id_hex)`. Projects a JSON object keyed
    // by `group_id_hex` → newest-N `MarmotMessageRow` array for every joined
    // group. Cheap: one lock + MDK SQLite reads per group, all already-
    // decrypted (no re-decrypt; `service().get_messages(&gid)` reads the
    // MDK SQLite message store directly). Bounded newest-N per group (see
    // `DEFAULT_MESSAGE_PAGE`). Rationale for the all-groups object shape: the
    // host view reads per-group tails from `projections["nmp.marmot.messages"][gid]`
    // without needing a "current active group" round-trip (D1: view-state stays
    // out of the kernel), and a new message in one group does not re-emit the
    // entire group list (clean edge-triggering vs `nmp.marmot.snapshot`).
    {
        let msgs_projection = Arc::clone(&projection);
        app_ref.register_snapshot_projection("nmp.marmot.messages", move || {
            msgs_projection
                .with_inner(|h| {
                    // Iterate every joined group and build the keyed map.
                    let group_ids: Vec<String> = h
                        .service()
                        .get_groups()
                        .map(|gs| {
                            gs.into_iter()
                                .map(|g| crate::projection::state::hex_encode(g.mls_group_id.as_slice()))
                                .collect()
                        })
                        .unwrap_or_default();
                    let mut map = serde_json::Map::with_capacity(group_ids.len());
                    for gid_hex in group_ids {
                        let rows = crate::projection::ops::group_messages(h, &gid_hex, DEFAULT_MESSAGE_PAGE);
                        map.insert(
                            gid_hex,
                            serde_json::to_value(rows).unwrap_or(serde_json::Value::Array(vec![])),
                        );
                    }
                    serde_json::Value::Object(map)
                })
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
        });
    }
    let observer_id = app_ref
        .register_event_observer(Arc::clone(&projection) as Arc<dyn nmp_core::KernelEventObserver>);
    if observer_id.0 == 0 {
        return std::ptr::null_mut(); // poisoned slot — soft fail.
    }

    let tap = Arc::new(MarmotIngestTap::new(Arc::clone(&projection)));
    let raw_observer_id = app_ref.register_raw_event_observer(
        MarmotIngestTap::kind_filter(),
        tap as Arc<dyn RawEventObserver>,
    );
    if raw_observer_id.0 == 0 {
        app_ref.unregister_event_observer(observer_id);
        return std::ptr::null_mut();
    }

    // Step 2: install the substrate-generic host-op handler against the
    // same `MarmotProjection` the observer + tap registered above are
    // tied to. The actor's `DispatchHostOp` arm pulls this handler from
    // the slot whenever the `MarmotActionModule::execute` body emits the
    // command — so every `nmp.marmot` dispatch reaches the SAME shared
    // projection state that `MarmotHandle::dispatch` (the in-process
    // Rust-native accessor) mutates and that the legacy bespoke
    // `nmp_marmot_dispatch` symbol used to mutate pre-PR-3 (one source of
    // truth; D4).
    //
    // A second `register_with_keys` (account switch, re-register) installs
    // a fresh handler over the new projection; `set_host_op_handler`
    // replaces the prior slot entry atomically.
    app_ref.set_host_op_handler(
        Arc::new(MarmotMlsOpHandler::new(Arc::clone(&projection)))
            as Arc<dyn nmp_core::substrate::HostOpHandler>,
    );

    // D7: the gift-wrap inbox subscription (kind:1059 `#p` filter, deterministic
    // id, account scope) is protocol policy — it lives in `nmp-marmot`, not in
    // this glue. The FFI only resolves the concrete pubkey and forwards.
    let pubkey_hex = keys.public_key().to_hex();
    app_ref.push_interest(crate::interest::giftwrap_inbox_interest(&pubkey_hex));

    Box::into_raw(Box::new(MarmotHandle {
        projection,
        observer_id,
        raw_observer_id,
        app,
    }))
}

/// Register a Marmot projection against `app`.
///
/// * `app` — the live `NmpApp` (from `nmp_app_new`). MUST outlive the
///   handle. NULL → null handle.
/// * `secret_key_hex` — **signer seam**: the local identity secret as hex
///   or `nsec…`. `MarmotService` signs key-package events and gift-wraps
///   with this key directly until a kernel `Keys` provider exists. NULL or
///   unparuseable → null handle.
/// * `db_dir` — the app-support directory; the DB is created at
///   `<db_dir>/marmot-mls-state.sqlite` (owned by this crate). NULL →
///   null handle.
///
/// Returns a non-null `*mut MarmotHandle` on success; `null` on any
/// failure (D6).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_marmot_register(
    app: *mut NmpApp,
    secret_key_hex: *const c_char,
    db_dir: *const c_char,
) -> *mut MarmotHandle {
    if app.is_null() {
        return std::ptr::null_mut();
    }
    let (Some(sk), Some(dir)) = (c_str_opt(secret_key_hex), c_str_opt(db_dir)) else {
        return std::ptr::null_mut();
    };
    let Ok(keys) = Keys::parse(&sk) else {
        return std::ptr::null_mut();
    };
    let db_path = format!("{}/marmot-mls-state.sqlite", dir.trim_end_matches('/'));
    register_with_keys(app, keys, &db_path)
}

/// Register a Marmot projection using the actor-owned active local key.
/// Swift never sees the secret — the key is read from the slot the actor
/// writes after every identity mutation. Returns a non-null handle on
/// success; `null` if no local account is active or `db_dir` is NULL (D6).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_marmot_register_active(
    app: *mut NmpApp,
    db_dir: *const c_char,
) -> *mut MarmotHandle {
    if app.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: app is non-null and valid for this call.
    let app_ref = unsafe { &*app };
    // ADR-0025 raw-nsec escape: Marmot's MLS state cannot be recovered
    // without the user's nsec, so the Marmot FFI bridge is the one
    // explicitly-allowed consumer of `mls_local_nsec`. The d13 Part-B
    // path-scope check exempts `crates/nmp-marmot/`, so no per-line
    // `doctrine-allow` is needed here after the step-12 return to `crates/`.
    let Some(sk) = app_ref.mls_local_nsec() else {
        return std::ptr::null_mut();
    };
    let Ok(keys) = Keys::parse(&sk) else {
        return std::ptr::null_mut();
    };
    let Some(dir) = c_str_opt(db_dir) else {
        return std::ptr::null_mut();
    };
    let db_path = format!("{}/marmot-mls-state.sqlite", dir.trim_end_matches('/'));
    let handle = register_with_keys(app, keys, &db_path);
    if !handle.is_null() && app_ref.take_pending_mls_autopublish() {
        publish_key_package_on_register(handle);
    }
    handle
}

/// JSON snapshot. Null handle / serialize failure → null (D6). Caller owns
/// the returned pointer until [`nmp_marmot_string_free`].
///
/// # Deprecation
///
/// Prefer the push projection `"nmp.marmot.snapshot"` (registered in
/// [`register_with_keys`] via [`nmp_ffi::NmpApp::register_snapshot_projection`],
/// ADR-0039). The push projection rides the SnapshotFrame edge-triggered on
/// `changed_since_emit` — no polling needed (D8).
///
/// This symbol is retained ONLY because `MarmotBridge.swift` still calls the
/// C-ABI export. It will be removed once that Swift consumer migrates to the
/// pushed frame (`projections["nmp.marmot.snapshot"]` in `apply()`).
///
/// Rust callers: use [`MarmotHandle::snapshot_rust`] instead.
#[deprecated(
    note = "use the `nmp.marmot.snapshot` push projection (ADR-0039); \
            this pull symbol will be removed once MarmotBridge.swift migrates"
)]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_marmot_snapshot(handle: *mut MarmotHandle) -> *mut c_char {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let snap = handle.projection.snapshot(now_secs());
    to_c_json(&snap)
}

/// Newest-N decrypted messages for the group whose MLS id is
/// `group_id_hex`. JSON array; `[]` on any soft failure (unknown group,
/// poisoned mutex, parse error). Null handle / serialize failure → null.
///
/// # Deprecation
///
/// Prefer the push projection `"nmp.marmot.messages"` (registered in
/// [`register_with_keys`] via [`nmp_ffi::NmpApp::register_snapshot_projection`],
/// ADR-0039). The push projection emits a JSON object keyed by `group_id_hex`
/// → newest-N message rows, riding the SnapshotFrame edge-triggered.
///
/// This symbol is retained ONLY because `MarmotBridge.swift` still calls the
/// C-ABI export. It will be removed once that Swift consumer migrates to the
/// pushed frame (`projections["nmp.marmot.messages"][group_id_hex]` in `apply()`).
///
/// Rust callers: use [`MarmotHandle::messages_rust`] instead.
#[deprecated(
    note = "use the `nmp.marmot.messages` push projection (ADR-0039); \
            this pull symbol will be removed once MarmotBridge.swift migrates"
)]
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_marmot_group_messages(
    handle: *mut MarmotHandle,
    group_id_hex: *const c_char,
) -> *mut c_char {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return std::ptr::null_mut();
    };
    let Some(gid_hex) = c_str_opt(group_id_hex) else {
        return to_c_string("[]");
    };
    let rows = handle
        .projection
        .with_inner(|h| {
            crate::projection::ops::group_messages(h, &gid_hex, DEFAULT_MESSAGE_PAGE)
        })
        .unwrap_or_default();
    match serde_json::to_string(&rows) {
        Ok(s) => to_c_string(&s),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a string previously returned by snapshot / group_messages /
/// dispatch. Null is a silent no-op.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_marmot_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: caller guarantees `ptr` came from `CString::into_raw` in one
    // of our string-returning symbols and has not been freed.
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

/// Drop the observer registration and free the handle. Idempotent: null is
/// a silent no-op. The handle MUST NOT be used after this call.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_marmot_unregister(handle: *mut MarmotHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: caller guarantees `handle` came from
    // `nmp_marmot_register` and has not already been freed.
    let boxed = unsafe { Box::from_raw(handle) };
    if !boxed.app.is_null() {
        // SAFETY: same `app` validity rule as register.
        let app_ref = unsafe { &*boxed.app };
        // Drop both kernel registrations (distinct slots): the lossy
        // metadata observer AND the raw inbound-ingest tap. Both are
        // idempotent no-ops for unknown ids (D6). Dropping the raw tap
        // releases the kernel's `Arc<dyn RawEventObserver>`, which in turn
        // releases the tap's `Arc<MarmotProjection>` clone — no
        // use-after-free of `app` (it is read only here, then `boxed`
        // drops).
        app_ref.unregister_event_observer(boxed.observer_id);
        app_ref.unregister_raw_event_observer(boxed.raw_observer_id);
    }
}

// ── helpers ──────────────────────────────────────────────────────────────

#[must_use]
pub(crate) fn c_str_opt(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees `ptr` (when non-null) is a valid
    // nul-terminated C string for the duration of this call.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(|s| s.to_owned())
}

fn to_c_string(s: &str) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

fn to_c_json<T: serde::Serialize>(v: &T) -> *mut c_char {
    match serde_json::to_string(v) {
        Ok(s) => to_c_string(&s),
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests;
