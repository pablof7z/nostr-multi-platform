// ffi-transport-bench/allocator.rs
//
// CountingAllocator — process-wide counting GlobalAlloc.
//
// IMPORTANT: The timing pass and the allocation pass are run in SEPARATE
// invocations.  Mixing them would confuse results: the atomic fetch_add
// overhead (2–5 ns per alloc) is non-trivial at sub-microsecond scale and
// would perturb timing measurements.
//
// The GlobalAlloc is always active (Rust requires a single global allocator),
// but callers only read the counters during the explicit allocation pass.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

#[global_allocator]
pub static GLOBAL: CountingAllocator = CountingAllocator;

pub struct CountingAllocator;

static TOTAL_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static TOTAL_ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            TOTAL_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            TOTAL_ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let ptr = System.realloc(ptr, layout, new_size);
        if !ptr.is_null() {
            TOTAL_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            TOTAL_ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        }
        ptr
    }
}

/// A snapshot of the allocation counters at a point in time.
#[derive(Clone, Copy, Debug, Default)]
pub struct AllocSnapshot {
    pub allocations: u64,
    pub allocated_bytes: u64,
}

pub fn alloc_snapshot() -> AllocSnapshot {
    AllocSnapshot {
        allocations: TOTAL_ALLOCATIONS.load(Ordering::Relaxed),
        allocated_bytes: TOTAL_ALLOCATED_BYTES.load(Ordering::Relaxed),
    }
}
