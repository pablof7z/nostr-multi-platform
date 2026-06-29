// ffi-transport-bench/lanes.rs
//
// Lane A (C-lane) and Lane B (UniFFI-lane) implementations.
//
// These are the two transports under comparison.  All lane-specific code
// (callback types, sink trait, delivery functions) lives here; main.rs wires
// the lanes together and runs the timing + alloc passes.

use nmp_native_runtime::UpdateListener;
use std::ffi::c_void;
use std::hint::black_box;
use std::sync::Arc;
use uniffi_core::Lower;

// ── Lane A: C-lane ────────────────────────────────────────────────────────────
//
// Replicates the exact closure shape from app_lifecycle_ffi.rs:
//
//   let listener = callback.map(|callback| {
//       let context = context as usize;
//       Arc::new(move |bytes: &[u8]| {
//           callback(context as *mut c_void, bytes.as_ptr(), bytes.len());
//       }) as nmp_native_runtime::UpdateListener
//   });
//
// Lane A additionally performs a mandatory shell-copy (one memcpy of the
// transient slice into an owned Vec) to make the comparison fair: the real
// contract says the slice is valid only for the callback duration, so every
// real host must copy once.

/// The extern "C" callback signature from app_lifecycle_ffi.rs.
pub type UpdateCallback = extern "C" fn(*mut c_void, *const u8, usize);

/// The shell-side receive buffer — simulates the host's owned copy.
/// In the real app this is the Swift/Kotlin Data / ByteArray built from the
/// transient slice.  Here it is an allocated Vec dropped at end of callback.
extern "C" fn c_lane_callback(context: *mut c_void, ptr: *const u8, len: usize) {
    // SAFETY: context is a dummy sentinel; we only read ptr/len, never store them.
    let _ = black_box(context);
    // Mandatory shell copy: models the host copying the transient slice before
    // forwarding to UI or storage.
    let owned: Vec<u8> = unsafe {
        let slice = std::slice::from_raw_parts(ptr, len);
        slice.to_vec()
    };
    black_box(owned);
}

/// Build a `UpdateListener` that drives Lane A.
pub fn build_c_lane_listener() -> UpdateListener {
    let callback: UpdateCallback = c_lane_callback;
    // Dummy context sentinel (same pattern as production code).
    let context: usize = 0xdeadbeef_usize;
    Arc::new(move |bytes: &[u8]| {
        callback(context as *mut c_void, bytes.as_ptr(), bytes.len());
    })
}

// ── Lane B: UniFFI lane ───────────────────────────────────────────────────────
//
// Exercises the real UniFFI lowering path:
//   1. `<Vec<u8> as Lower<UniFfiTag>>::lower(data)` — genuine FfiConverter:
//      allocates a Vec<u8> scratch buffer, writes 4-byte i32 length prefix +
//      each byte via `write()`, wraps in `RustBuffer::from_vec()`.
//   2. An indirect vtable dispatch into a Rust foreign-trait callback stub.
//   3. The stub performs the lower-bound foreign consume: copies RustBuffer
//      contents into an owned Vec (mimicking Swift Data / Kotlin ByteArray),
//      then drops (frees) the RustBuffer.
//
// SYNTHETIC: the stub's memcpy is a LOWER BOUND of the real foreign consume.
// Real ARC/GC bookkeeping, JNI local-ref table management, and dispatch-queue
// hop are NOT modeled.  The pre-registered 3x surcharge in report.rs
// compensates.

/// Foreign-trait callback interface used by Lane B.
/// This is the Rust side of what UniFFI would auto-generate as a callback
/// interface VTable.  The vtable dispatch is the indirect fn-pointer call
/// through a `Box<dyn UpdateFrameSink>` trait object.
pub trait UpdateFrameSink: Send + Sync {
    /// Called with the lowered RustBuffer.  The sink owns the buffer and must
    /// free it (drop it back into Rust ownership) to avoid leaks.
    fn on_frame(&self, buf: uniffi_core::RustBuffer);
}

/// Lower-bound foreign-consume stub.
///
/// SYNTHETIC (explicitly labeled): this models the floor cost of what Swift/
/// Kotlin must do when receiving a UniFFI `Vec<u8>` callback:
///   1. Copy the RustBuffer bytes into a managed heap allocation (Swift Data /
///      Kotlin ByteArray).  Modeled as `buf.destroy_into_vec()`.
///   2. ARC/GC overhead — NOT modeled; pre-registered 3x surcharge compensates.
///   3. JNI boundary surcharge — NOT modeled; same 3x band.
pub struct LowerBoundForeignSink;

// Dummy UniFFI type tag — required by FfiConverter generic parameter.
struct UniFfiTag;

impl UpdateFrameSink for LowerBoundForeignSink {
    #[inline(never)]
    fn on_frame(&self, buf: uniffi_core::RustBuffer) {
        // Lower-bound foreign consume: reclaim the RustBuffer into an owned
        // Vec<u8> and immediately drop it.  Floor cost of the real
        // managed-runtime allocation + copy.  SYNTHETIC.
        let owned = black_box(buf.destroy_into_vec());
        drop(owned);
    }
}

/// Run one UniFFI-lane delivery of a single frame.
///
/// 1. Clones the frame data (owned Vec<u8> for the lowering path).
/// 2. `Lower::lower` — REAL uniffi_core lowering (alloc scratch buf, write
///    4-byte length + bytes, wrap in RustBuffer::from_vec).
/// 3. Indirect vtable dispatch into the foreign-consume stub.
/// 4. Stub consumes the RustBuffer (lower-bound foreign copy + free).
#[inline(never)]
pub fn uniffi_lane_deliver(sink: &dyn UpdateFrameSink, frame: &[u8]) {
    let data: Vec<u8> = frame.to_vec();
    let rust_buf = <Vec<u8> as Lower<UniFfiTag>>::lower(data);
    sink.on_frame(rust_buf);
}
