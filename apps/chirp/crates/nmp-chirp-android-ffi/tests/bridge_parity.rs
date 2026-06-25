use std::ffi::c_char;

const KOTLIN_BRIDGE: &str =
    include_str!("../../../../../apps/chirp/android/app/src/main/java/org/nmp/android/KernelBridge.kt");
const LIB_RS: &str = include_str!("../src/lib.rs");
const ACTION_RS: &str = include_str!("../src/action.rs");
const PLATFORM_RS: &str = include_str!("../src/platform.rs");
const SIGNER_RS: &str = include_str!("../src/signer.rs");
const EXTERNAL_SIGNER_RS: &str = include_str!("../src/external_signer.rs");
// The NIP-55 signer-request push-listener JNI symbols
// (`nativeSetSignerRequestListener`/`nativeClearSignerRequestListener`) live in
// this module, not `lib.rs`, so the parity grep must include it.
const SIGNER_REQUEST_LISTENER_RS: &str = include_str!("../src/signer_request_listener.rs");

#[test]
fn android_bridge_declares_parity_jni_symbols() {
    let rust = [
        LIB_RS,
        ACTION_RS,
        PLATFORM_RS,
        SIGNER_RS,
        EXTERNAL_SIGNER_RS,
        SIGNER_REQUEST_LISTENER_RS,
    ]
    .join("\n");
    for method in [
        "SetStoragePath",
        "LifecycleForeground",
        "LifecycleBackground",
        "IsAlive",
        "AckActionStage",
        "LoadOlderFeed",
        "SignInBunker",
        "CancelBunkerHandshake",
        "NostrConnectUri",
        // ADR-0048 Stage 2 — NIP-55 external-signer seam. Issue #1284 migrated
        // the request path from the `NextSignerRequest` blocking drain to the
        // `SetSignerRequestListener`/`ClearSignerRequestListener` JNI push
        // callbacks (D8 — no polling), mirroring the update-listener seam.
        "SignInNip55",
        "DeliverSignerResponse",
        "SetSignerRequestListener",
        "ClearSignerRequestListener",
    ] {
        let kotlin_decl = format!("native{method}(");
        assert!(
            KOTLIN_BRIDGE.contains(&kotlin_decl),
            "KernelBridge.kt is missing {kotlin_decl}",
        );

        let rust_symbol = format!("Java_org_nmp_android_KernelBridge_native{method}");
        assert!(
            rust.contains(&rust_symbol),
            "nmp-chirp-android-ffi is missing {rust_symbol}",
        );
    }
}

#[test]
fn rust_path_reexports_cover_android_parity_surface() {
    let _ =
        nmp_ffi::nmp_app_set_storage_path as extern "C" fn(*mut nmp_ffi::NmpApp, *const c_char) -> u32;
    let _ = nmp_ffi::nmp_app_lifecycle_foreground as extern "C" fn(*mut nmp_ffi::NmpApp);
    let _ = nmp_ffi::nmp_app_lifecycle_background as extern "C" fn(*mut nmp_ffi::NmpApp);
    let _ = nmp_ffi::nmp_app_is_alive as extern "C" fn(*mut nmp_ffi::NmpApp) -> u8;
    let _ = nmp_ffi::nmp_app_ack_action_stage as extern "C" fn(*mut nmp_ffi::NmpApp, *const c_char);
    let _ = nmp_ffi::nmp_app_load_older_feed as extern "C" fn(*mut nmp_ffi::NmpApp, *const c_char);
    let _ =
        nmp_ffi::nmp_app_signin_bunker as extern "C" fn(*mut nmp_ffi::NmpApp, *const c_char, u8);
    let _ = nmp_app_chirp::nmp_signer_broker_init as extern "C" fn(*mut nmp_ffi::NmpApp) -> u32;
    let _ = nmp_app_chirp::nmp_app_cancel_bunker_handshake as extern "C" fn(*mut nmp_ffi::NmpApp);
    let _ = nmp_app_chirp::nmp_app_nostrconnect_uri
        as extern "C" fn(*mut nmp_ffi::NmpApp, *const c_char) -> *mut c_char;
    let _ = nmp_ffi::nmp_free_string as extern "C" fn(*mut c_char);
    // ADR-0048 Stage 2 — NIP-55 external-signer driver surface.
    let _ = nmp_ffi::nmp_external_signer_init as extern "C" fn(*mut nmp_ffi::NmpApp);
    let _ =
        nmp_ffi::nmp_app_signin_nip55 as extern "C" fn(*mut nmp_ffi::NmpApp, *const c_char);
    let _ = nmp_ffi::nmp_app_deliver_external_signer_response
        as extern "C" fn(*mut nmp_ffi::NmpApp, *const c_char);
}
