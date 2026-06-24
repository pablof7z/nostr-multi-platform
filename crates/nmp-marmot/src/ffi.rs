//! Marmot (MLS-over-Nostr) per-app FFI surface.
//!
//! Native links against two `extern "C"` symbols, neither of which carries
//! secret key material across the ABI (#1727 / ADR-0025):
//! - [`nmp_marmot_register_active`] builds the service from the actor-owned
//!   local-key slot, registers the observer, ingest parsers, and push
//!   projections, then returns an opaque `*mut MarmotHandle`.
//! - [`nmp_marmot_unregister`] drops those registrations and frees the handle.
//!
//! Plus the Rust-internal (NOT `extern "C"`) [`register_with_secret_hex`] —
//! same registration with an in-hand secret for the app-shell nsec sign-in
//! path; the secret never re-crosses the ABI (#1727).
//!
//! The former pull symbols `nmp_marmot_snapshot`, `nmp_marmot_group_messages`,
//! and `nmp_marmot_string_free` were deleted in V-107 (ADR-0039). Swift now
//! reads Marmot state reactively from the pushed `nmp.marmot.snapshot` /
//! `nmp.marmot.messages` SnapshotFrame projections instead.
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
//! ## Relay seams — CLOSED
//!
//! Both directions are closed in [`crate::projection`]: ops publish their
//! signed events INTERNALLY via [`crate::projection::publish`] (no Swift relay
//! path ever existed), and inbound events arrive via the per-kind
//! `IngestParser` below. See `crate::projection::ops` / `state` for the
//! per-kind routing + pending-commit detail.
//!
//! ## Inbound ingest seam — CLOSED
//!
//! Registration installs per-kind `IngestParser` registrations for the active
//! Marmot kinds (`[444, 445, 1059, 30443]`) under the `"marmot"` slot. The
//! kernel delivers accepted verified events to
//! [`crate::projection::tap::MarmotIngestParser`], which reconstructs the
//! signed `nostr::Event` from [`nmp_store::VerifiedEvent::raw`] and drives
//! `ops::ingest_signed_event_core`, so relay-delivered welcomes/messages
//! surface in the next snapshot with no Swift involvement.
//! `nmp_marmot_unregister` tears down BOTH kernel registrations (the lossy
//! `KernelEvent` metadata observer AND all per-kind `IngestParser` slots via
//! `unregister_ingest_parser`). This was the last open seam (raw-tap PR-2).

use std::ffi::{c_char, CStr};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use nmp_core::KernelEventObserverId;
use nmp_ffi::NmpApp;
use nostr::Keys;
use serde_json::{json, Value};

use crate::service::MarmotService;

use crate::projection::action::{MarmotAction, MarmotActionModule};
use crate::projection::handler::MarmotMlsOpHandler;
use crate::projection::state::MarmotProjection;
use crate::projection::tap::{MarmotIngestParser, MARMOT_INGEST_SLOT, TAP_KINDS};

/// Page size used by the `nmp.marmot.messages` push projection and
/// [`MarmotHandle::messages_rust`].
const DEFAULT_MESSAGE_PAGE: usize = 200;
/// Generic key-id for the MLS SQLite DB encryption key in the keyring.
/// The service-id (app-scoped namespace) is caller-supplied (D0 #1606).
pub(crate) const KEYRING_DB_KEY_ID: &str = "marmot-mls-db-key";

mod slot;
use slot::register_marmot_snapshot_projections;
pub use slot::{MarmotProjectionSlot, MarmotSlotState};

/// Opaque handle returned by the registration entry points. Boxed so the
/// address is stable; Swift holds the raw pointer until
/// [`nmp_marmot_unregister`].
pub struct MarmotHandle {
    projection: Arc<MarmotProjection>,
    /// Shared slot the push-projection closures read from (ADR-0039, V-107,
    /// #1651). Set to [`MarmotSlotState::Cleared`] in `nmp_marmot_unregister` so
    /// the closures emit nothing until the next account registers. On account
    /// switch the new `register_with_keys` both replaces the closures by key AND
    /// installs a fresh `Ready` slot — the replace-by-key path alone already
    /// suffices for the re-register case; the slot clear handles
    /// sign-out-without-re-register. (`InitFailed` registrations have no handle,
    /// so they do not own a `MarmotHandle::projection_slot`.)
    projection_slot: MarmotProjectionSlot,
    /// Lossy `KernelEvent` observer (key-package metadata tracker — see
    /// `MarmotProjection::on_kernel_event`). Torn down in `unregister`.
    observer_id: KernelEventObserverId,
    /// The inbound ingest seam is now per-kind `IngestParser` registrations
    /// (raw-tap PR-2) under the `"marmot"` slot key. No id to store — teardown
    /// calls `unregister_ingest_parser(kind, MARMOT_INGEST_SLOT)` for each of
    /// `TAP_KINDS`. Account-switch re-registration is handled atomically by the
    /// slot-keyed `replace_kind_parser` in `register_with_keys`.
    pub(crate) app: *mut NmpApp,
}

// SAFETY: identical rationale to `ChirpHandle` (see `crate::ffi`). The
// auto-derived `!Send`/`!Sync` comes only from `app: *mut NmpApp`; the
// `Arc<MarmotProjection>` is already `Send + Sync`. The honest invariant:
//
//   1. Swift owns this handle and only reaches the FFI entry points below
//      from `@MainActor` types (`KernelModel` / `MarmotStore`), so the
//      handle struct itself is never raced (a documented Swift caller
//      convention, not a type guarantee).
//   2. The `Arc<MarmotProjection>` IS shared across threads — the kernel
//      actor thread runs `MarmotProjection::on_kernel_event` and
//      `MarmotIngestParser::parse` while the Swift main actor calls
//      `snapshot()` / dispatch. Soundness of that sharing comes from
//      `MarmotProjection`'s interior `Mutex<Inner>`, not from this
//      `unsafe impl`.
//   3. The `app` raw pointer is only read (to forward fire-and-forget
//      kernel commands). No use-after-free is possible: `nmp_app_free`'s
//      `NmpApp::Drop` sends `Shutdown` and `join()`s the actor thread
//      before freeing the allocation, and every kernel callback that can
//      reach `app` (`on_kernel_event`, `parse`) runs INLINE on that actor
//      thread — the join fences them.
//
// CALLER CONTRACT: `nmp_app_free` must not run while a kernel callback that
// reaches this projection is still executing. The in-process Rust-trait
// registration path used here (`register_event_observer` /
// `replace_ingest_parser`) gets that fence from the actor join.
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
    /// Rust callers use this directly. Swift consumers read from the pushed
    /// SnapshotFrame projection key (`projections["nmp.marmot.snapshot"]`).
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
    /// Rust callers use this directly. Swift consumers read from the pushed
    /// SnapshotFrame projection key (`projections["nmp.marmot.messages"]`).
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
    /// accessor parses the JSON into the SAME typed
    /// [`crate::projection::action::MarmotAction`] that the kernel actor's
    /// `HostOpCommand` handler dispatches, then invokes
    /// [`crate::projection::ops::dispatch`] without going through any FFI.
    ///
    /// ## D0 / layering
    ///
    /// This is a Rust-native method on a `pub` opaque handle in this app
    /// crate. It is NOT a C-ABI symbol, not part of any host FFI surface,
    /// and not subject to ADR-0025's bespoke-FFI prohibition (which
    /// targeted `extern "C"` cluster bloat in the iOS bridge).
    pub fn dispatch(&self, action: &Value) -> Value {
        let action: MarmotAction = match serde_json::from_value(action.clone()) {
            Ok(action) => action,
            Err(e) => {
                return json!({
                    "ok": false,
                    "error": format!("invalid MarmotAction: {e}"),
                });
            }
        };
        // `correlation_id` is `None`: this in-process path (REPL / TUI / tests)
        // has no action-registry correlation, so the deferred-pending path
        // stays off (callers get the old terminal soft-fail); it activates only
        // for the typed `HostOpCommand` pipeline, which supplies an id.
        self.projection
            .with_inner(|h| crate::projection::ops::dispatch(h, &action, now_secs(), None))
            .unwrap_or_else(|| {
                json!({
                    "ok": false,
                    "error": "projection mutex poisoned",
                })
            })
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Inner registration logic shared by `register_with_secret_hex` and
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
///   `MarmotService::new` fails, return a null handle. #1651: the failure is no
///   longer stderr-only — the `nmp.marmot.snapshot` projection is registered
///   with a degraded `MarmotInitError::DbKeyLost` slot so the host observes the
///   error as kernel state and may surface a recovery prompt or retry.
/// - If `initialize()` already chose the mock store (`use_mock = true`)
///   the service init failing is also fatal (same `DbKeyLost` surfacing).
/// - The mock store is ONLY legitimately installed when `initialize()` chose
///   it (non-Apple platform or Apple platform with no Keychain entitlement).
///   In that case the SUCCESS path sets `MarmotInitError::KeyringUnavailable`
///   on the projection so the snapshot surfaces the diagnostic to the host.
///
/// `keyring_service_id` — app-owned namespace for the MLS DB encryption key
/// (D0 #1606: the reusable crate must not embed a Chirp-specific string).
pub(crate) fn register_with_keys(
    app: *mut NmpApp,
    keys: Keys,
    db_path: &str,
    keyring_service_id: &str,
) -> *mut MarmotHandle {
    // Capability slot for the probe (rationale in `credential_store::initialize`).
    // SAFETY: both callers null-check `app` before delegating here.
    let capability_slot = unsafe { &*app }.capability_callback_slot();
    let Some(use_mock) = crate::credential_store::initialize(capability_slot, keyring_service_id)
    else {
        return std::ptr::null_mut();
    };

    // V-62: `use_mock` is `true` only when `initialize()` chose the mock store
    // (env hatch / probe failure); surfaced as `MarmotInitError::KeyringUnavailable`
    // on the success path below.
    let service =
        match MarmotService::new(db_path, keyring_service_id, KEYRING_DB_KEY_ID, keys.clone()) {
            Ok(s) => s,
            Err(e) => {
                // #1651: `MarmotService::new` failed — typically the encrypted MLS
                // SQLite DB exists but its keyring encryption key was lost
                // ("Database exists … but no encryption key found in keyring …"),
                // so no usable service exists. Classify every service-init `Err` as
                // `DbKeyLost{detail}` (the keyring-UNAVAILABLE case is a SUCCESS path
                // with the mock store — `use_mock` below — never an `Err` here).
                //
                // Instead of returning a bare null handle (the failure was previously
                // stderr-only), register the `nmp.marmot.snapshot` projection against
                // the kernel with a degraded `InitFailed` slot so the failure surfaces
                // as kernel-owned typed state the shells render. The handle is still
                // null (no usable service), but the state is now visible AND a later
                // `register_with_keys` (the user recovered the key) replaces it.
                // Classify: the lost-encryption-key case (the recoverability /
                // permanent-data-loss problem) is surfaced as `DbKeyLost`; every
                // OTHER service-init failure (disk full, unwritable path, …) is
                // `Other`, so the shell does NOT show the scary "data is
                // unrecoverable" copy for a benign/transient error.
                let init_error = slot::classify_service_init_error(&e.to_string());
                eprintln!(
                    "nmp-marmot: service init failed (use_mock={use_mock}): {e}; \
                 surfacing {init_error:?} in nmp.marmot.snapshot, returning null handle"
                );
                // SAFETY: caller guarantees `app` is non-null and valid.
                let app_ref = unsafe { &*app };
                let slot: MarmotProjectionSlot =
                    Arc::new(Mutex::new(MarmotSlotState::InitFailed(init_error)));
                register_marmot_snapshot_projections(app_ref, &slot);
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
    unsafe { &mut *app }
        .register_action(MarmotActionModule)
        .expect("duplicate registration: nmp-marmot MarmotActionModule"); // doctrine-allow: D6 — startup-only call; RegistrationError here is a programmer error

    // SAFETY: caller guarantees `app` is non-null and valid.
    let app_ref = unsafe { &*app };
    // V-62 / #1651: when `initialize()` chose the in-memory mock store
    // (`use_mock == true`) the service works but its secrets are not durable,
    // so the projection carries `MarmotInitError::KeyringUnavailable` in every
    // snapshot. The host reads `snapshot.init_error` and may block group
    // features or prompt the user to resolve the Keychain issue.
    let init_error =
        use_mock.then_some(crate::projection::payload::MarmotInitError::KeyringUnavailable);
    let projection = Arc::new(MarmotProjection::new(service, init_error));
    projection.set_app(app);

    // V-107 / ADR-0039: register the two Marmot push projections onto the
    // canonical snapshot seam. Both ride the SnapshotFrame on every tick
    // that `changed_since_emit` is set — no polling (D8). The kernel marks
    // `changed_since_emit` on every accepted inbound relay event (including
    // kind:445/1059 handled by the raw ingest tap), so new messages / welcomes
    // surface in the next pushed frame edge-triggered.
    //
    // **Lifecycle / account-switch correctness (D1, no stale data):**
    // Closures capture a `MarmotProjectionSlot` (`Arc<Mutex<MarmotSlotState>>`)
    // rather than a bare `Arc<MarmotProjection>`. The slot mirrors the wallet
    // projection pattern (`wallet_runtime.rs:146`):
    // - On sign-out (`nmp_marmot_unregister`): the slot is set to
    //   `MarmotSlotState::Cleared`, so the closures emit nothing until a new
    //   account registers.
    // - On account switch (a new `register_with_keys` call): the closures are
    //   replaced by key (the registry is HashMap::insert / last-writer-wins)
    //   AND a fresh `Ready` slot is installed for the new account. Both legs
    //   handle the switch independently. #1651: the same path also replaces a
    //   prior `InitFailed` registration when the user recovers the keyring key.
    //
    // `register_typed_snapshot_projection` is lock-and-push; calling it here
    // (post-construction, before or after `nmp_app_start`) is the documented
    // safe pattern (the slot is `Arc<Mutex<_>>`). The slot holds `Ready` for a
    // live service; the SAME `register_marmot_snapshot_projections` helper that
    // the `InitFailed` path above uses registers both push-projection keys, so
    // there is one registration code path for every outcome (#1651).
    let projection_slot: MarmotProjectionSlot =
        Arc::new(Mutex::new(MarmotSlotState::Ready(Arc::clone(&projection))));
    register_marmot_snapshot_projections(app_ref, &projection_slot);

    let observer_id = app_ref
        .register_event_observer(Arc::clone(&projection) as Arc<dyn nmp_core::KernelEventObserver>);
    if observer_id.0 == 0 {
        return std::ptr::null_mut(); // poisoned slot — soft fail.
    }

    // Register the `IngestParser` for every Marmot kind under the `"marmot"`
    // slot key (raw-tap PR-2). `replace_kind_parser` semantics: a subsequent
    // `register_with_keys` call (account switch) atomically evicts the prior
    // parser for each kind and installs a fresh one bound to the new projection,
    // without touching the NIP-17 DM inbox parser on kind:1059 (distinct slot
    // key `"nip17.dm_inbox"`).
    //
    // No error path: if the dispatcher lock is poisoned (D6 silent no-op), the
    // event observer registered above would still function; Marmot would
    // simply not receive ingest events. This matches the prior raw-observer
    // behaviour on poison.
    let parser = Arc::new(MarmotIngestParser::new(Arc::clone(&projection)));
    for &kind in TAP_KINDS.iter() {
        app_ref.replace_ingest_parser(
            kind,
            MARMOT_INGEST_SLOT,
            Arc::clone(&parser) as Arc<dyn nmp_core::substrate::IngestParser>,
        );
    }

    // Step 2: install the substrate-generic host-op handler against the
    // same `MarmotProjection` the observer + parser registered above are
    // tied to. The `HostOpCommand` (on the `Protocol` arm) clones this handler
    // out of the slot whenever the `MarmotActionModule::execute` body emits the
    // command — so every `nmp.marmot` dispatch reaches the SAME shared
    // projection state that `MarmotHandle::dispatch` (the in-process
    // Rust-native accessor) mutates and that the legacy bespoke
    // `nmp_marmot_dispatch` symbol used to mutate pre-PR-3 (one source of
    // truth; D4).
    //
    // A second `register_with_keys` (account switch, re-register) installs
    // a fresh handler over the new projection; `set_host_op_handler`
    // replaces the prior slot entry atomically.
    app_ref.set_host_op_handler(Arc::new(MarmotMlsOpHandler::new(Arc::clone(&projection)))
        as Arc<dyn nmp_core::substrate::HostOpHandler>);

    // D7: the gift-wrap inbox subscription (kind:1059 `#p` filter, deterministic
    // id, account scope) is protocol policy — it lives in `nmp-marmot`, not in
    // this glue. The FFI only resolves the concrete pubkey and forwards.
    let pubkey_hex = keys.public_key().to_hex();
    app_ref.ensure_interest(
        crate::interest::giftwrap_inbox_identity(&pubkey_hex),
        crate::interest::giftwrap_inbox_interest(&pubkey_hex),
    );

    // Post-restart live-receive fix (re-push per-group kind:445; see resubscribe.rs).
    projection.resubscribe_all_groups();

    let handle = Box::into_raw(Box::new(MarmotHandle {
        projection,
        projection_slot,
        observer_id,
        app,
    }));

    // Shared autopublish tail (PR-4) — see `autopublish` module.
    autopublish::maybe_autopublish_on_register(app_ref, handle);

    handle
}

/// Register a Marmot projection against `app` using an **in-hand** secret key.
///
/// A plain `pub` Rust function — **NOT** an `extern "C"` symbol (#1727). No
/// native code registers Marmot with a raw secret; native uses
/// [`nmp_marmot_register_active`] (reads the actor-owned `mls_local_nsec` slot,
/// ADR-0025). This exists only for the app-shell nsec sign-in path, where
/// `nmp_app_signin_nsec` enqueues `AddSigner` asynchronously so the slot is not
/// yet populated when registration must run synchronously — the in-hand secret
/// sidesteps that race and never re-crosses the C/JNI ABI.
///
/// `app` MUST outlive the handle; `secret_key_hex` is the local identity secret
/// (hex or `nsec…`) `MarmotService` signs/gift-wraps with; `db_dir` is the
/// app-support dir (DB at `<db_dir>/marmot-mls-state.sqlite`);
/// `keyring_service_id` is the app-owned MLS DB-key namespace (non-empty). Any
/// NULL / unparseable / empty argument degrades to a null handle (D6).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn register_with_secret_hex(
    app: *mut NmpApp,
    secret_key_hex: *const c_char,
    db_dir: *const c_char,
    keyring_service_id: *const c_char,
) -> *mut MarmotHandle {
    if app.is_null() {
        return std::ptr::null_mut();
    }
    let (Some(sk), Some(dir), Some(svc_id)) = (
        c_str_opt(secret_key_hex),
        c_str_opt(db_dir),
        c_str_opt(keyring_service_id),
    ) else {
        return std::ptr::null_mut();
    };
    if svc_id.is_empty() {
        return std::ptr::null_mut();
    }
    let Ok(keys) = Keys::parse(&sk) else {
        return std::ptr::null_mut();
    };
    let db_path = format!("{}/marmot-mls-state.sqlite", dir.trim_end_matches('/'));
    register_with_keys(app, keys, &db_path, &svc_id)
}

/// Register a Marmot projection using the actor-owned active local key.
/// Swift never sees the secret — the key is read from the slot the actor
/// writes after every identity mutation. Returns a non-null handle on
/// success; `null` if no local account is active, `db_dir` is NULL, or
/// `keyring_service_id` is NULL/empty (D6).
///
/// * `keyring_service_id` — app-owned namespace for the MLS DB encryption key;
///   non-empty (see [`register_with_secret_hex`]).
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_marmot_register_active(
    app: *mut NmpApp,
    db_dir: *const c_char,
    keyring_service_id: *const c_char,
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
    let (Some(dir), Some(svc_id)) = (c_str_opt(db_dir), c_str_opt(keyring_service_id)) else {
        return std::ptr::null_mut();
    };
    if svc_id.is_empty() {
        return std::ptr::null_mut();
    }
    let db_path = format!("{}/marmot-mls-state.sqlite", dir.trim_end_matches('/'));
    // Autopublish is handled in the shared register_with_keys tail.
    register_with_keys(app, keys, &db_path, &svc_id)
}

/// Drop the observer registration and free the handle. Idempotent: null is
/// a silent no-op. The handle MUST NOT be used after this call.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn nmp_marmot_unregister(handle: *mut MarmotHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: caller guarantees `handle` came from a registration entry point
    // (`nmp_marmot_register_active` / `register_with_secret_hex`) and has not
    // already been freed.
    let boxed = unsafe { Box::from_raw(handle) };

    // V-107 / ADR-0039: clear the projection slot so the push-projection
    // closures (`nmp.marmot.snapshot` / `nmp.marmot.messages`) emit nothing
    // for subsequent snapshot frames rather than stale data from the
    // signed-out account. A D6 no-op if the mutex is poisoned.
    if let Ok(mut slot) = boxed.projection_slot.lock() {
        *slot = MarmotSlotState::Cleared;
    }

    if !boxed.app.is_null() {
        // SAFETY: same `app` validity rule as register.
        let app_ref = unsafe { &*boxed.app };
        // Tear down the lossy KernelEvent metadata observer.
        app_ref.unregister_event_observer(boxed.observer_id);
        // Remove all per-kind `IngestParser` slots registered under the
        // `"marmot"` slot key (raw-tap PR-2). Each `unregister_ingest_parser`
        // call is idempotent — a D6 no-op when the slot is already absent or
        // when the dispatcher lock is poisoned. Dropping the slot entries
        // releases the kernel's `Arc<dyn IngestParser>`, which in turn
        // releases the parser's `Arc<MarmotProjection>` clone — no
        // use-after-free of `app` (it is read only here, then `boxed` drops).
        for &kind in TAP_KINDS.iter() {
            app_ref.unregister_ingest_parser(kind, MARMOT_INGEST_SLOT);
        }
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

mod autopublish;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod keyring_identity_tests;

#[cfg(test)]
mod autopublish_tests;

#[cfg(test)]
mod deferred_kp_tests;

#[cfg(test)]
mod deferred_snapshot_tests;

#[cfg(test)]
mod init_error_tests;
