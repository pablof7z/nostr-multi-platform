//! In-process + OS metric capture.
//!
//! - RSS: `task_info(MACH_TASK_BASIC_INFO)` on macOS, `/proc/self/statm` on
//!   Linux — the same mach pattern ffi-stress/ffi.rs uses (reused, not edited).
//! - Allocations: the CountingAllocator pattern from reactivity-bench/allocator.rs
//!   (this binary installs its own `#[global_allocator]` — one per binary).
//! - CPU% / per-thread CPU: captured by the OS sidecar (`scripts/perf-sanity/`)
//!   and merged here from its JSON, because per-thread CPU is naturally a `top
//!   -H`/`ps` job, not an in-process counter.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

// ── CountingAllocator (pattern copied from reactivity-bench/allocator.rs) ─────

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

struct CountingAllocator;

static TOTAL_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static CURRENT_HEAP_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_HEAP_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            TOTAL_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            bump_heap(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        CURRENT_HEAP_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

fn bump_heap(additional: usize) {
    let current = CURRENT_HEAP_BYTES.fetch_add(additional, Ordering::Relaxed) + additional;
    let mut peak = PEAK_HEAP_BYTES.load(Ordering::Relaxed);
    while current > peak {
        match PEAK_HEAP_BYTES.compare_exchange_weak(
            peak,
            current,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(observed) => peak = observed,
        }
    }
}

#[derive(Clone, Copy)]
pub struct AllocSnapshot {
    pub allocations: u64,
    pub current_heap_bytes: usize,
    pub peak_heap_bytes: usize,
}

pub fn alloc_snapshot() -> AllocSnapshot {
    AllocSnapshot {
        allocations: TOTAL_ALLOCATIONS.load(Ordering::Relaxed),
        current_heap_bytes: CURRENT_HEAP_BYTES.load(Ordering::Relaxed),
        peak_heap_bytes: PEAK_HEAP_BYTES.load(Ordering::Relaxed),
    }
}

// ── RSS (process resident set) ────────────────────────────────────────────────

/// Current process RSS in bytes. macOS via mach `task_info`; Linux via
/// `/proc/self/statm`. Returns 0 on unsupported platforms.
pub fn process_rss_bytes() -> u64 {
    #[cfg(target_os = "macos")]
    {
        macos_rss()
    }
    #[cfg(target_os = "linux")]
    {
        linux_rss()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        0
    }
}

pub fn process_rss_mb() -> f64 {
    process_rss_bytes() as f64 / (1024.0 * 1024.0)
}

#[cfg(target_os = "macos")]
fn macos_rss() -> u64 {
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

#[cfg(target_os = "linux")]
fn linux_rss() -> u64 {
    // statm fields are in pages; field 2 (index 1) is resident.
    let Ok(statm) = std::fs::read_to_string("/proc/self/statm") else {
        return 0;
    };
    let mut parts = statm.split_whitespace();
    let _total = parts.next();
    let resident_pages: u64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let page = unsafe { libc_sysconf_pagesize() };
    resident_pages.saturating_mul(page)
}

#[cfg(target_os = "linux")]
unsafe fn libc_sysconf_pagesize() -> u64 {
    extern "C" {
        fn sysconf(name: i32) -> i64;
    }
    const SC_PAGESIZE: i32 = 30;
    let v = sysconf(SC_PAGESIZE);
    if v <= 0 {
        4096
    } else {
        v as u64
    }
}

// ── OS sidecar JSON merge ─────────────────────────────────────────────────────

/// CPU/RSS samples captured by the `scripts/perf-sanity/` orchestrator. The
/// sidecar writes one JSON object per phase keyed by phase name.
#[derive(Clone, Debug, Default)]
pub struct OsPhaseMetrics {
    /// Mean process CPU% over the phase (`ps -o %cpu` / `top` sampling).
    pub cpu_pct_mean: Option<f64>,
    /// Peak process CPU% over the phase.
    pub cpu_pct_peak: Option<f64>,
    /// Highest single-thread CPU% seen (`top -H` / `ps -M`).
    pub max_thread_cpu_pct: Option<f64>,
    /// Peak RSS in MB (`/usr/bin/time -l` or sampled `ps -o rss`).
    pub rss_peak_mb: Option<f64>,
    /// RSS slope in MB/hr (least-squares over the sampled `ps -o rss` loop).
    pub rss_slope_mb_per_hr: Option<f64>,
}

/// Parse the OS sidecar JSON (a flat `{ "<phase>": { ...fields... } }` map)
/// without pulling in serde derive machinery for the dynamic shape — uses
/// `serde_json::Value` so the sidecar schema can evolve independently.
pub fn load_os_metrics(path: &str, phase: &str) -> Option<OsPhaseMetrics> {
    let raw = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let obj = value.get(phase)?;
    let num = |k: &str| obj.get(k).and_then(serde_json::Value::as_f64);
    Some(OsPhaseMetrics {
        cpu_pct_mean: num("cpu_pct_mean"),
        cpu_pct_peak: num("cpu_pct_peak"),
        max_thread_cpu_pct: num("max_thread_cpu_pct"),
        rss_peak_mb: num("rss_peak_mb"),
        rss_slope_mb_per_hr: num("rss_slope_mb_per_hr"),
    })
}

/// Percentile helper (nearest-rank) over an unsorted slice of millis.
pub fn percentile(samples: &mut [f64], pct: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = ((pct / 100.0) * (samples.len() as f64 - 1.0)).round() as usize;
    samples[rank.min(samples.len() - 1)]
}
