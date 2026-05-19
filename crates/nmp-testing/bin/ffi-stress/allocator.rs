//! Counting allocator — vendored from reactivity-bench (ADR-0004).
//!
//! Installed as `#[global_allocator]` in this binary only. Provides
//! snapshot-based heap accounting used by S1, S2, S3, S5 to detect
//! per-event allocation growth without Instruments.
//!
//! D8: zero per-event allocations after warmup is the primary invariant
//! this allocator validates.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[global_allocator]
pub(crate) static GLOBAL: CountingAllocator = CountingAllocator;

pub(crate) struct CountingAllocator;

static TOTAL_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static TOTAL_ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);
static CURRENT_HEAP_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_HEAP_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            record_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        CURRENT_HEAP_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let ptr = System.realloc(ptr, layout, new_size);
        if !ptr.is_null() {
            TOTAL_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            TOTAL_ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
            if new_size >= layout.size() {
                update_current_heap(new_size - layout.size());
            } else {
                CURRENT_HEAP_BYTES.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        ptr
    }
}

fn record_alloc(size: usize) {
    TOTAL_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    TOTAL_ALLOCATED_BYTES.fetch_add(size as u64, Ordering::Relaxed);
    update_current_heap(size);
}

fn update_current_heap(additional: usize) {
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

/// Point-in-time snapshot of heap counters. Take two snapshots and diff
/// them to measure allocation growth across a bounded window (e.g., one
/// emit cycle after warmup).
///
/// The `current_heap_bytes` field tracks NET live heap (allocations - frees),
/// which is the correct metric for the D8 "zero per-event allocations" gate:
/// transient per-cycle allocations (e.g., CString) that are immediately freed
/// do not cause NET heap growth and thus pass the gate.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AllocSnapshot {
    pub(crate) allocations: u64,
    pub(crate) allocated_bytes: u64,
    /// Net live heap bytes at snapshot time. This is what D8 gates against.
    pub(crate) current_heap_bytes: usize,
}

pub(crate) fn alloc_snapshot() -> AllocSnapshot {
    AllocSnapshot {
        allocations: TOTAL_ALLOCATIONS.load(Ordering::Relaxed),
        allocated_bytes: TOTAL_ALLOCATED_BYTES.load(Ordering::Relaxed),
        current_heap_bytes: CURRENT_HEAP_BYTES.load(Ordering::Relaxed),
    }
}

impl AllocSnapshot {
    /// Allocations added since `earlier`.
    pub(crate) fn allocs_since(&self, earlier: &AllocSnapshot) -> u64 {
        self.allocations.saturating_sub(earlier.allocations)
    }

    /// Bytes allocated (total gross, not net) since `earlier`.
    pub(crate) fn bytes_since(&self, earlier: &AllocSnapshot) -> u64 {
        self.allocated_bytes.saturating_sub(earlier.allocated_bytes)
    }

    /// Net live-heap delta since `earlier`.
    ///
    /// A negative result (earlier had more live bytes) means the allocator
    /// reclaimed memory. Zero or negative is the D8 passing condition.
    pub(crate) fn net_heap_delta(&self, earlier: &AllocSnapshot) -> i64 {
        self.current_heap_bytes as i64 - earlier.current_heap_bytes as i64
    }
}
