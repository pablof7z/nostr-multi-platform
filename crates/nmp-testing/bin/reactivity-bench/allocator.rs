use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

struct CountingAllocator;

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

#[derive(Clone, Copy)]
pub(crate) struct AllocationSnapshot {
    pub(crate) allocations: u64,
    pub(crate) allocated_bytes: u64,
    pub(crate) peak_heap_bytes: usize,
}

pub(crate) fn allocation_snapshot() -> AllocationSnapshot {
    AllocationSnapshot {
        allocations: TOTAL_ALLOCATIONS.load(Ordering::Relaxed),
        allocated_bytes: TOTAL_ALLOCATED_BYTES.load(Ordering::Relaxed),
        peak_heap_bytes: PEAK_HEAP_BYTES.load(Ordering::Relaxed),
    }
}
