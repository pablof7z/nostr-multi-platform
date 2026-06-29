//! Thin wrapper shims for the nmp-stress harness, replacing the former
//! `nmp_ffi` re-exports with direct calls to `nmp-native-runtime`.
//!
//! The harness calls these with C-ABI-style signatures (raw pointers, CStr,
//! etc.) because the scenario code was written against the old nmp-ffi surface.
//! The wrappers preserve those exact signatures so the scenarios compile
//! unchanged.

use std::ffi::{c_int, c_void, CStr};
use std::sync::Arc;

pub(crate) use nmp_native_runtime::NmpApp;

/// Allocate a new `NmpApp` instance on the heap and return a raw pointer.
/// The caller owns the pointer and must eventually pass it to `nmp_app_free`.
pub(crate) fn nmp_app_new() -> *mut NmpApp {
    Box::into_raw(Box::new(nmp_native_runtime::new_app()))
}

/// Release an `NmpApp` previously allocated with `nmp_app_new`.
/// No-op on null.
pub(crate) fn nmp_app_free(app: *mut NmpApp) {
    if !app.is_null() {
        // SAFETY: pointer was created by Box::into_raw(Box::new(...)) in nmp_app_new.
        unsafe { &*app }.stop_runtime();
        // SAFETY: pointer was created by Box::into_raw(Box::new(...)) in nmp_app_new.
        unsafe { drop(Box::from_raw(app)) };
    }
}

pub(crate) fn new_app_ptr() -> *mut NmpApp {
    nmp_app_new()
}

pub(crate) fn free_app_ptr(app: *mut NmpApp) {
    nmp_app_free(app);
}

/// Set the visible_limit and emit_hz without starting the actor threads.
pub(crate) fn nmp_app_configure(app: *mut NmpApp, visible_limit: usize, emit_hz: u32) {
    if app.is_null() {
        return;
    }
    // SAFETY: app is a valid non-null pointer from nmp_app_new.
    unsafe { &*app }.configure_runtime(visible_limit, emit_hz);
}

pub(crate) fn configure_app(app: *mut NmpApp, visible_limit: usize, emit_hz: u32) {
    nmp_app_configure(app, visible_limit, emit_hz);
}

/// Install or remove the snapshot update listener.
///
/// Keeps the legacy `(ctx, Some(extern "C" fn))` signature so the
/// existing scenario code compiles unchanged.
pub(crate) fn nmp_app_set_update_callback(
    app: *mut NmpApp,
    ctx: *mut c_void,
    callback: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
) {
    if app.is_null() {
        return;
    }
    match callback {
        Some(cb) => {
            let ctx_usize = ctx as usize;
            // SAFETY: the scenarios keep the app alive for at least as long as
            // the listener is installed.
            unsafe { &*app }.set_update_listener(Some(Arc::new(move |bytes: &[u8]| {
                cb(ctx_usize as *mut c_void, bytes.as_ptr(), bytes.len());
            })));
        }
        None => {
            unsafe { &*app }.set_update_listener(None);
        }
    }
}

pub(crate) fn set_update_listener(
    app: *mut NmpApp,
    ctx: *mut c_void,
    callback: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
) {
    nmp_app_set_update_callback(app, ctx, callback);
}

/// Declare incremental-apply capability (ADR-0055 Rung 3).
pub(crate) fn nmp_app_declare_incremental_apply(app: *mut NmpApp) -> i32 {
    if app.is_null() {
        return -1;
    }
    if unsafe { &*app }.declare_incremental_apply().is_ok() {
        0
    } else {
        -1
    }
}

/// Inject `count` real Schnorr-signed kind-1 events (test-support path).
pub(crate) fn nmp_app_inject_signed_events(app: *mut NmpApp, base_created_at: u64, count: u32) {
    use nostr::{EventBuilder, Keys, Timestamp};
    use nmp_core::actor::{ActorCommand, TestSupportCommand};

    if app.is_null() {
        return;
    }
    let keys = Keys::generate();
    let events: Vec<nmp_store::VerifiedEvent> = (0..count as u64)
        .filter_map(|i| {
            let ts = Timestamp::from(base_created_at.saturating_add(i));
            let nostr_event = EventBuilder::text_note(format!("signed harness event {i}"))
                .custom_created_at(ts)
                .sign_with_keys(&keys)
                .ok()?;
            let raw = nmp_store::RawEvent {
                id: nostr_event.id.to_hex(),
                pubkey: nostr_event.pubkey.to_hex(),
                created_at: nostr_event.created_at.as_secs(),
                kind: nostr_event.kind.as_u16() as u32,
                tags: nostr_event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                content: nostr_event.content.clone(),
                sig: nostr_event.sig.to_string(),
            };
            nmp_store::VerifiedEvent::try_from_raw(raw).ok()
        })
        .collect();
    // SAFETY: app is non-null.
    unsafe { &*app }.send_cmd(ActorCommand::TestSupport(
        TestSupportCommand::IngestPreVerifiedEvents(events),
    ));
}

/// Inject `count` pre-verified (unchecked) kind-1 events (test-support path).
#[allow(dead_code)]
pub(crate) fn nmp_app_inject_pre_verified_events(
    app: *mut NmpApp,
    base_id_prefix: *const std::ffi::c_char,
    base_created_at: u64,
    count: u32,
) {
    use nmp_core::actor::{ActorCommand, TestSupportCommand};

    if app.is_null() {
        return;
    }
    let prefix = if base_id_prefix.is_null() {
        "stress".to_string()
    } else {
        unsafe { CStr::from_ptr(base_id_prefix) }
            .to_str()
            .unwrap_or("stress")
            .to_string()
    };
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
    let events: Vec<nmp_store::VerifiedEvent> = (0..count as u64)
        .map(|i| {
            let raw_id = format!("{prefix}{i:0>16x}");
            let id = format!("{raw_id:0<64}");
            let id = id[..64].to_string();
            let pubkey = POOL[(i as usize) % POOL.len()].to_string();
            let raw = nmp_store::RawEvent {
                id,
                pubkey,
                created_at: base_created_at.saturating_add(i),
                kind: 1,
                tags: Vec::new(),
                content: format!("harness event {i}"),
                sig: "0".repeat(128),
            };
            nmp_store::VerifiedEvent::from_raw_unchecked(raw)
        })
        .collect();
    // SAFETY: app is non-null.
    unsafe { &*app }.send_cmd(ActorCommand::TestSupport(
        TestSupportCommand::IngestPreVerifiedEvents(events),
    ));
}

/// Read actor command-lane stats (queue depth + cumulative drops).
pub(crate) fn nmp_app_read_command_lane_stats(
    app: *mut NmpApp,
    out_depth: *mut u64,
    out_drops: *mut u64,
) {
    if app.is_null() {
        return;
    }
    // SAFETY: app is non-null; out pointers are caller-guaranteed valid.
    let app_ref = unsafe { &*app };
    if !out_depth.is_null() {
        unsafe { *out_depth = app_ref.queue_depth_for_test() };
    }
    if !out_drops.is_null() {
        unsafe { *out_drops = app_ref.command_drops_for_test() };
    }
}

/// Resolve a reference (profile or event) through the kernel's ref-resolution seam.
pub(crate) fn nmp_app_resolve_ref(
    app: *mut NmpApp,
    namespace: c_int,
    key: *const std::ffi::c_char,
    consumer_id: *const std::ffi::c_char,
    shape: c_int,
    liveness: c_int,
) {
    if app.is_null() || key.is_null() || consumer_id.is_null() {
        return;
    }
    let ns = match namespace {
        0 => nmp_core::RefNamespace::Profile,
        1 => nmp_core::RefNamespace::Event,
        _ => return,
    };
    let shape_val = match shape {
        0 => nmp_core::RefShape::Profile(nmp_core::ProfileShape::Ref),
        1 => nmp_core::RefShape::Profile(nmp_core::ProfileShape::Card),
        2 => nmp_core::RefShape::Event(nmp_core::EventShape::Embed),
        3 => nmp_core::RefShape::Event(nmp_core::EventShape::Raw),
        _ => return,
    };
    let liveness_val = nmp_core::RefLiveness::from_ffi(liveness);
    let key_str = match unsafe { CStr::from_ptr(key) }.to_str() {
        Ok(s) => s.to_owned(),
        Err(_) => return,
    };
    let consumer_str = match unsafe { CStr::from_ptr(consumer_id) }.to_str() {
        Ok(s) => s.to_owned(),
        Err(_) => return,
    };
    // SAFETY: app is non-null.
    unsafe { &*app }.resolve_ref(ns, key_str, consumer_str, shape_val, liveness_val);
}

/// Release a previously-resolved reference.
pub(crate) fn nmp_app_release_ref(
    app: *mut NmpApp,
    namespace: c_int,
    key: *const std::ffi::c_char,
    consumer_id: *const std::ffi::c_char,
) {
    if app.is_null() || key.is_null() || consumer_id.is_null() {
        return;
    }
    let ns = match namespace {
        0 => nmp_core::RefNamespace::Profile,
        1 => nmp_core::RefNamespace::Event,
        _ => return,
    };
    let key_str = match unsafe { CStr::from_ptr(key) }.to_str() {
        Ok(s) => s.to_owned(),
        Err(_) => return,
    };
    let consumer_str = match unsafe { CStr::from_ptr(consumer_id) }.to_str() {
        Ok(s) => s.to_owned(),
        Err(_) => return,
    };
    // SAFETY: app is non-null.
    unsafe { &*app }.release_ref(ns, key_str, consumer_str);
}

/// Read process-global projection churn stats.
/// Both output pointers may be null.
pub(crate) fn nmp_app_read_projection_churn_stats(
    out_serialized: *mut u64,
    out_changed: *mut u64,
) {
    use std::sync::atomic::Ordering;
    if !out_serialized.is_null() {
        unsafe {
            *out_serialized =
                nmp_core::testing::PROCESS_PROJECTIONS_SERIALIZED.load(Ordering::Relaxed);
        }
    }
    if !out_changed.is_null() {
        unsafe {
            *out_changed =
                nmp_core::testing::PROCESS_PROJECTIONS_CHANGED.load(Ordering::Relaxed);
        }
    }
}

/// Generate N deterministic lowercase 64-hex-char pubkeys suitable for all
/// FFI calls that require `is_hex_pubkey` validation to pass.
pub(crate) fn test_pubkeys(count: usize) -> Vec<std::ffi::CString> {
    (0..count)
        .map(|i| {
            let hex = format!("{:0>16x}{:0>16x}{:0>16x}{:0>16x}", i, i + 1, i + 2, i + 3);
            std::ffi::CString::new(hex).expect("no interior nuls in hex string")
        })
        .collect()
}

/// Read current process RSS in bytes.
/// On macOS uses `task_info(MACH_TASK_BASIC_INFO)`. Returns 0 elsewhere.
pub(crate) fn process_rss_bytes() -> u64 {
    #[cfg(target_os = "macos")]
    {
        use std::mem;

        #[repr(C)]
        #[derive(Default)]
        struct MachTaskBasicInfo {
            virtual_size: u64,
            resident_size: u64,
            resident_size_max: u64,
            user_time_seconds: u32,
            user_time_microseconds: u32,
            system_time_seconds: u32,
            system_time_microseconds: u32,
            policy: i32,
            suspend_count: i32,
        }

        extern "C" {
            fn task_self_trap() -> u32;
            fn task_info(
                target_task: u32,
                flavor: u32,
                task_info_out: *mut u32,
                task_info_out_cnt: *mut u32,
            ) -> i32;
        }

        const MACH_TASK_BASIC_INFO: u32 = 20;
        let mut info = MachTaskBasicInfo::default();
        let mut count = (mem::size_of::<MachTaskBasicInfo>() / mem::size_of::<u32>()) as u32;
        let ret = unsafe {
            task_info(
                task_self_trap(),
                MACH_TASK_BASIC_INFO,
                &mut info as *mut MachTaskBasicInfo as *mut u32,
                &mut count,
            )
        };
        if ret == 0 { info.resident_size } else { 0 }
    }
    #[cfg(not(target_os = "macos"))]
    {
        0
    }
}
