//! Chirp-owned Android JNI delivery surface (`nmp-chirp-android-ffi`).
//!
//! This crate is the Layer-6 cdylib for the Chirp Android app. It depends
//! explicitly on `nmp-app-chirp` and `nmp-chirp-config` (Chirp operator
//! policy); those are intentional Chirp-only deps that belong here, not in a
//! generic framework crate (issue #1611, D0, `docs/architecture/crate-boundaries.md`
//! §10).
//!
//! JNI shim: Android ⇄ the nmp-core kernel via Rust-path function calls.
//!
//! Doctrine: no business logic or cached state here (D5/D8) — pure transport.
//! Errors never cross FFI (D6): the kernel reports via update frames; these
//! entrypoints return only a handle / bytes / void. The kernel's update
//! callback fires on its own listener thread with a pointer valid ONLY for the
//! call's duration (`docs/ffi-surface.md` §3); the `on_update` trampoline in
//! `session.rs` copies it into owned bytes and pushes them straight to a
//! registered Kotlin listener via JNI (`nativeSetUpdateListener`, issue #614 —
//! D8 no-polling). This mirrors the iOS push model: Kotlin no longer drains a
//! 250 ms-timed channel on a blocked thread. Init-only configuration calls
//! may return explicit status codes so late wiring is visible to the host.

use std::ffi::CString;
use std::sync::Arc;

use jni::objects::{JClass, JString};
use jni::sys::{jint, jlong};
use jni::JNIEnv;

use nmp_app_chirp::{
    action_spec_json_for_intent, nmp_app_chirp_declare_consumed_projections,
    nmp_app_chirp_register, nmp_signer_broker_init, NmpRegisterStatus,
};

mod action;
mod capability;
mod claims;
mod external_signer;
mod flat_feed;
mod identity;
mod marmot;
mod platform;
mod relay_seeding;
mod session;
mod signer;
mod signer_request_listener;
mod update_listener;
use nmp_app_chirp::nmp_app_chirp_create_new_account;
use nmp_ffi::{
    nmp_app_add_relay, nmp_app_declare_incremental_apply, nmp_app_encode_profile, nmp_app_free,
    nmp_app_new, nmp_app_remove_account, nmp_app_remove_relay, nmp_app_signin_nsec, nmp_app_start,
    nmp_app_stop, nmp_app_switch_active, nmp_free_string, NmpConfigStatus,
};
use session::{insert_session, remove_session};
pub(crate) use session::{session_arc, Session};

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeNew(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    let app = nmp_app_new();
    if app.is_null() {
        return 0;
    }
    let broker_rc = nmp_signer_broker_init(app);
    if broker_rc != NmpConfigStatus::Ok as u32 {
        eprintln!(
            "nmp_signer_broker_init failed: rc={broker_rc} (1=NullApp, 2=AlreadyStarted, 3=Unavailable)"
        );
        nmp_app_free(app);
        return 0;
    }
    // ADR-0053 / Workstream-E4 — declare Chirp's projection-consumption intent.
    // Chirp is a full client, so this is the explicit `consume_all` (Chirp reads
    // every kernel built-in); see `nmp_app_chirp_declare_consumed_projections` in
    // nmp-app-chirp. Must run before `nmp_app_start` — an undeclared start is a
    // loud forgotten-wiring bug, not a silent firehose. Thin: one static call.
    nmp_app_chirp_declare_consumed_projections(app);
    // ADR-0055 R3-S4 — declare that this host implements the incremental-apply
    // contract (D3-3/D3-4/D3-5). The kernel switches from full-snapshot mode to
    // delta mode after this call. Must run after declare_consumed_projections and
    // before nmp_app_start. Non-zero return is a hard init error.
    let rc = nmp_app_declare_incremental_apply(app);
    if rc != 0 {
        // RegistryUnavailable (2) or AlreadyStarted (1) — neither should occur
        // here (called before start, registry is fresh). Abort; returning 0 would
        // leave the kernel in an undefined incremental state.
        // rc legend: 0=Ok, 1=AlreadyStarted, 2=RegistryUnavailable, -1=null-app.
        eprintln!(
            "nmp_app_declare_incremental_apply failed: rc={rc} (1=AlreadyStarted, 2=RegistryUnavailable)"
        );
        // The Session that would otherwise own `app` and free it via
        // `free_native` is never constructed on this path, so free the kernel
        // here to avoid leaking the `nmp_app_new` allocation.
        nmp_app_free(app);
        return 0;
    }
    // V-73: null viewer_pubkey (no viewer set at startup) always succeeds.
    // Android passes null until the user signs in; the status is expected to
    // be Ok.  If registration fails for an unexpected reason, fall back to a
    // null chirp handle — the Session is still created so the kernel remains
    // usable; the missing Chirp handle degrades the home feed gracefully (D6).
    let mut chirp = std::ptr::null_mut();
    let _register_status = nmp_app_chirp_register(app, std::ptr::null(), &mut chirp);
    debug_assert_eq!(
        _register_status,
        NmpRegisterStatus::Ok as u32,
        "nmp_app_chirp_register with null viewer must succeed"
    );
    let session = Arc::new(Session::new(app, chirp));
    let handle = insert_session(session);
    // ADR-0048 Stage 2 — register the external-signer capability trampoline
    // (context = registry handle id, assigned above) + init the NIP-55 driver.
    if handle != 0 {
        external_signer::install(app, handle);
    }
    handle
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeStart(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    visible_limit: jint,
    emit_hz: jint,
) {
    if let Some(s) = session_arc(handle) {
        s.with_app(|app| {
            nmp_app_start(app, visible_limit as u32, emit_hz as u32);
        });
    }
}

/// Seed the relay list from a JSON string override or the Chirp defaults.
///
/// `relays_json` is an optional JSON array of `["url", "role"]` pairs
/// (e.g. `[["ws://127.0.0.1:10547","both"]]`). When `null` (normal
/// production path) the Chirp reference relays are seeded instead.
/// When non-null the supplied list REPLACES the defaults entirely —
/// no merging is performed.
///
/// Parsing and policy live in Rust (D7). Kotlin ferries the raw string
/// provided by the test harness unchanged (thin-shell principle).
///
/// D6: null/dead handle, a null relays_json, or a relays_json that fails
/// to parse falls back to the Chirp reference relay set so the kernel is
/// never left without any relay.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeSeedRelays(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    relays_json: JString,
) {
    let Some(s) = session_arc(handle) else { return };
    let override_json: Option<String> = {
        let obj: &jni::objects::JObject = AsRef::<jni::objects::JObject>::as_ref(&relays_json);
        if obj.as_raw().is_null() {
            None
        } else {
            env.get_string(&relays_json)
                .ok()
                .map(|s| s.to_string_lossy().into_owned())
        }
    };
    s.with_app(|app| {
        if let Some(json) = override_json.as_deref() {
            if relay_seeding::seed_relays_from_json(app, json) {
                return; // successfully seeded from override
            }
            // Malformed JSON: fall through to defaults (D6).
        }
        relay_seeding::seed_default_relays(app);
    });
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeClose(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_arc(handle) {
        s.close_updates();
    }
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeCreateLocalAccount(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    display_name: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let name = env
        .get_string(&display_name)
        .map(|s| s.to_string_lossy().into_owned())
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Android User".to_string());
    // nmp_app_chirp_create_new_account expects:
    //   profile_json = {"name":"…"}
    //   relays_json  = [["url","role"],…]  (Vec<(String,String)> serde shape)
    // The Chirp wrapper (not the generic nmp_app_create_new_account) injects
    // Chirp's product seed follows from nmp-chirp-config in Rust (#1493).
    let profile_json = format!(r#"{{"name":"{}"}}"#, name.replace('"', ""));
    let relays_json = relay_seeding::default_relays_json_array();
    let (Ok(profile_c), Ok(relays_c)) = (CString::new(profile_json), CString::new(relays_json))
    else {
        return;
    };
    s.with_app(|app| {
        nmp_app_chirp_create_new_account(app, profile_c.as_ptr(), relays_c.as_ptr(), false, 1);
    });
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeStop(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = session_arc(handle) {
        s.with_app(|app| nmp_app_stop(app));
    }
}

// The app-local profile/event JNI adapters live in `claims.rs` to keep this
// file under the AGENTS.md size cap (#984 split).

/// Encode a hex pubkey as a NIP-19 display identifier (`nprofile1…` or
/// `npub1…`). Wraps the existing `nmp_app_encode_profile` C-ABI symbol —
/// no new NMP C-ABI surface.
///
/// Returns a Kotlin `String` (or `null` when the handle is dead / the
/// pubkey is unusable). Follows the same `*mut c_char` → `jstring` pattern
/// as `nativeNostrConnectUri` in `signer.rs`.
///
/// D6: a null handle or a malformed pubkey degrades gracefully (returns
/// `null` — the Kotlin caller falls back to its own short-hex rendering).
/// Never panics across the JNI seam.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeEncodeProfile(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    pubkey: JString,
) -> jni::sys::jstring {
    use std::ffi::CStr;
    use std::ptr;
    let Some(s) = session_arc(handle) else {
        return ptr::null_mut();
    };
    let Some(pubkey_c) = jstring_to_cstring(&mut env, &pubkey) else {
        return ptr::null_mut();
    };
    let Some(raw_ptr) = s.with_app(|app| nmp_app_encode_profile(app, pubkey_c.as_ptr())) else {
        return ptr::null_mut();
    };
    if raw_ptr.is_null() {
        return ptr::null_mut();
    }
    let encoded = unsafe { CStr::from_ptr(raw_ptr) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: `nmp_free_string` is the canonical free for C-strings
    // allocated by any NMP FFI function (nmp-ffi/src/free.rs).
    nmp_free_string(raw_ptr);
    env.new_string(encoded)
        .map(|s| s.into_raw())
        .unwrap_or(ptr::null_mut())
}

/// Build a Chirp action dispatch spec from typed user intent.
///
/// Kotlin passes user intent only. Rust owns the action namespace and body JSON
/// shape returned as `{"namespace":"...","body_json":"..."}` or
/// `{"error":"..."}`.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeBuildActionSpec(
    mut env: JNIEnv,
    _class: JClass,
    intent_json: JString,
) -> jni::sys::jstring {
    let Some(intent) = env
        .get_string(&intent_json)
        .map(|s| s.to_string_lossy().into_owned())
        .ok()
        .filter(|s| !s.trim().is_empty())
    else {
        // D6: null on JNI failure — never panic through extern "system".
        return env
            .new_string(r#"{"error":"missing Chirp action intent JSON"}"#)
            .map(|s| s.into_raw())
            .unwrap_or(std::ptr::null_mut());
    };
    let result = action_spec_json_for_intent(&intent);
    env.new_string(&result)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Add a relay by URL and role string ("read", "write", or "both").
///
/// D6: null handle, null URL, or null role is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeAddRelay(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    url: JString,
    role: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(url) = jstring_to_cstring(&mut env, &url) else {
        return;
    };
    let Some(role) = jstring_to_cstring(&mut env, &role) else {
        return;
    };
    s.with_app(|app| nmp_app_add_relay(app, url.as_ptr(), role.as_ptr()));
}

/// Remove a relay by URL.
///
/// D6: null handle or null URL is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeRemoveRelay(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    url: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(url) = jstring_to_cstring(&mut env, &url) else {
        return;
    };
    s.with_app(|app| nmp_app_remove_relay(app, url.as_ptr()));
}

/// Sign in with an nsec secret key.
///
/// D6: null handle or invalid secret is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeSignInNsec(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    secret: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(secret) = jstring_to_cstring(&mut env, &secret) else {
        return;
    };
    s.with_app(|app| nmp_app_signin_nsec(app, secret.as_ptr(), 1));
}

/// Switch the active account to the given pubkey.
///
/// D6: null handle or invalid pubkey is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeSwitchAccount(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    pubkey: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(pubkey) = jstring_to_cstring(&mut env, &pubkey) else {
        return;
    };
    s.with_app(|app| nmp_app_switch_active(app, pubkey.as_ptr()));
}

/// Remove an account by pubkey.
///
/// D6: null handle or invalid pubkey is a silent no-op.
#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeRemoveAccount(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    pubkey: JString,
) {
    let Some(s) = session_arc(handle) else {
        return;
    };
    let Some(pubkey) = jstring_to_cstring(&mut env, &pubkey) else {
        return;
    };
    s.with_app(|app| nmp_app_remove_account(app, pubkey.as_ptr()));
}

#[no_mangle]
pub extern "system" fn Java_org_nmp_android_KernelBridge_nativeFree(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if let Some(s) = remove_session(handle) {
        // `free_native` quiesces the update callback (the gate blocks until any
        // in-flight `on_update` returns) and drops the JNI push listener
        // `GlobalRef` before reclaiming the kernel. The `Arc<Session>` held by
        // the registry keeps native state alive across concurrent JNI calls.
        s.free_native();
    }
}

/// Copy a Java `JString` into an owned `CString` ready for handing across the
/// C-ABI seam. Returns `None` if the `JString` was null or the JNI fetch
/// failed; `nmp_app_*` shims downstream of this treat `None` as a silent
/// no-op (D6).
pub(crate) fn jstring_to_cstring(env: &mut JNIEnv, value: &JString) -> Option<CString> {
    let java_str = env.get_string(value).ok()?;
    let owned = java_str.to_string_lossy().into_owned();
    CString::new(owned).ok()
}
