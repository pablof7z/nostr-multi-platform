//! Thin re-exports of the 14 `nmp_app_*` entry-points from `nmp-core`.
//!
//! The harness calls these directly via the Rust rlib dependency (the
//! alternative path from `harness.md` §1.6). The functions take `*mut NmpApp`
//! in their Rust signatures; Swift sees them as `*mut c_void` via the C ABI.
//! Both paths exercise identical FFI-surface code paths.
//!
//! The symbols remain `#[no_mangle] pub extern "C"` on the nmp-core side so
//! they are still reachable from Swift/C unchanged.

pub(crate) use nmp_ffi::{
    nmp_app_claim_profile, nmp_app_close_author, nmp_app_configure, nmp_app_free,
    nmp_app_inject_signed_events, nmp_app_new, nmp_app_open_author, nmp_app_release_profile,
    nmp_app_set_update_callback, NmpApp,
};
// nmp_app_inject_pre_verified_events is retained for possible future harness use
// but S3/S4/S5 all use nmp_app_inject_signed_events (T44 round-4).
#[allow(unused_imports)]
pub(crate) use nmp_ffi::nmp_app_inject_pre_verified_events;
// nmp_app_open_firehose_tag is retained for possible future use.
#[allow(unused_imports)]
pub(crate) use nmp_ffi::nmp_app_open_firehose_tag;

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
