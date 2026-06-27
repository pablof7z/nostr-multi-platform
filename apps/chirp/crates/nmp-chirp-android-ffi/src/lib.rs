//! Chirp-owned Android JNI/UniFFI delivery surface (`nmp-chirp-android-ffi`).
//!
//! This crate is the Layer-6 cdylib for the Chirp Android app. It depends
//! explicitly on `nmp-app-chirp` and `nmp-chirp-config` (Chirp operator
//! policy); those are intentional Chirp-only deps that belong here, not in a
//! generic framework crate (issue #1611, D0, `docs/architecture/crate-boundaries.md`
//! §10).
//!
//! ## Lane overview
//!
//! * **UniFFI app-loop lane** (M14-0 / issue #2129) — `uniffi_app_loop.rs`:
//!   `AppHandle` object with typed `DispatchAck` record and `UpdateSink`
//!   callback interface. Replaces the deleted JNI app-loop symbols:
//!   `nativeNew`, `nativeStart`, `nativeStop`, `nativeClose`, `nativeFree`,
//!   `nativeSetUpdateListener`, `nativeClearUpdateListener`,
//!   `nativeDispatchIntentBytes`, `nativeDispatchActionBytes`.
//!
//! * **Residual JNI lanes** (staged for future migration):
//!   signer, external-signer/NIP-55, capability, identity, marmot, platform,
//!   action stage-ack/retry/cancel, flat-feed, claims, relay management,
//!   account management, session utility helpers.
//!
//! Doctrine: no business logic or cached state here (D5/D8) — pure transport.
//! Errors never cross FFI (D6): the kernel reports via update frames. Init-only
//! configuration calls may return explicit status codes so late wiring is
//! visible to the host.

// UniFFI scaffolding for the app-loop lane (M14-0 / issue #2129).
// Namespace "nmp_android_ffi" matches [lib] name in Cargo.toml and
// cdylib_name in uniffi.toml.  Must be called exactly once per library.
uniffi::setup_scaffolding!("nmp_android_ffi");

use std::ffi::CString;

use jni::objects::{JClass, JString};
use jni::sys::jlong;
use jni::JNIEnv;

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
mod uniffi_app_loop;
mod update_listener;
use nmp_app_chirp::nmp_app_chirp_create_new_account;
use nmp_ffi::{
    nmp_app_add_relay, nmp_app_encode_profile, nmp_app_remove_account, nmp_app_remove_relay,
    nmp_app_signin_nsec, nmp_app_switch_active, nmp_free_string,
};
pub(crate) use session::{session_arc, Session};

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

/// Copy a Java `JString` into an owned `CString` ready for handing across the
/// C-ABI seam. Returns `None` if the `JString` was null or the JNI fetch
/// failed; `nmp_app_*` shims downstream of this treat `None` as a silent
/// no-op (D6).
pub(crate) fn jstring_to_cstring(env: &mut JNIEnv, value: &JString) -> Option<CString> {
    let java_str = env.get_string(value).ok()?;
    let owned = java_str.to_string_lossy().into_owned();
    CString::new(owned).ok()
}
