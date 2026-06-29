//! Thin local adapters for ffi-stress scenarios.

// V-68 / V-112 (ADR-0042): nmp_app_open_author, nmp_app_close_author,
// nmp_app_open_thread, nmp_app_close_thread deleted from nmp-ffi.
// ADR-0063 Lane H: nmp_app_claim_profile / nmp_app_release_profile deleted;
// harnesses migrated to nmp_app_resolve_ref / nmp_app_release_ref.
use std::ffi::c_void;
use std::sync::Arc;

pub(crate) use nmp_ffi::{
    nmp_app_inject_signed_events, nmp_app_read_command_lane_stats, nmp_app_release_ref,
    nmp_app_resolve_ref,
};
pub(crate) use nmp_native_runtime::NmpApp;
// ADR-0055 Rung 3 S5 — needed by the S6 Phase B measurement to enable
// incremental-apply on the second NmpApp instance.
#[allow(unused_imports)]
pub(crate) use nmp_ffi::nmp_app_declare_incremental_apply;
// nmp_app_inject_pre_verified_events is retained for possible future harness use
// but S3/S4/S5 all use nmp_app_inject_signed_events (T44 round-4).
#[allow(unused_imports)]
pub(crate) use nmp_ffi::nmp_app_inject_pre_verified_events;

pub(crate) fn new_app_ptr() -> *mut NmpApp {
    Box::into_raw(Box::new(nmp_native_runtime::new_app()))
}

pub(crate) fn free_app_ptr(app: *mut NmpApp) {
    if app.is_null() {
        return;
    }
    unsafe {
        (&*app).stop_runtime();
        drop(Box::from_raw(app));
    }
}

pub(crate) fn configure_app(app: *mut NmpApp, visible_limit: usize, emit_hz: u32) {
    if app.is_null() {
        return;
    }
    unsafe { (&*app).configure_runtime(visible_limit, emit_hz) };
}

pub(crate) fn set_update_listener(
    app: *mut NmpApp,
    ctx: *mut c_void,
    callback: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
) {
    if app.is_null() {
        return;
    }
    let listener = callback.map(|callback| {
        let ctx_addr = ctx as usize;
        Arc::new(move |payload: &[u8]| {
            callback(ctx_addr as *mut c_void, payload.as_ptr(), payload.len());
        }) as nmp_native_runtime::UpdateListener
    });
    unsafe { (&*app).set_update_listener(listener) };
}

/// Generate N deterministic lowercase 64-hex-char pubkeys suitable for all
/// FFI calls that require `is_hex_pubkey` validation to pass.
pub(crate) fn test_pubkeys(count: usize) -> Vec<std::ffi::CString> {
    (0..count)
        .map(|i| {
            // 64 hex chars derived from index — valid by construction.
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
        if ret == 0 {
            info.resident_size
        } else {
            0
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        0
    }
}
