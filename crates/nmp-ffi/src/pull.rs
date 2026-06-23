//! ADR-0058 §3 (step 3b) — synchronous, read-only FFI pull-page surface.
//!
//! [`nmp_mirror_pull_page`] is the C-ABI door a host (a nostrdb mirror, or a feed's
//! "give me more" cursor) uses to drain the kernel's raw ingest log. It is a
//! **read-only** call — it never dispatches an actor command, never mutates the
//! registry, never invokes a callback — so it does not cross the fire-and-forget
//! actor seam. ADR-0039 stays intact: this reads the raw event log, NOT a
//! kernel-derived projection (no `nmp_app_get_snapshot`, no projection-pull
//! accessor).
//!
//! ## Lock order (the load-bearing re-entrancy contract, ADR-0058 §3)
//!
//! 1. Read-lock the cursor registry, clone the [`PullCursorRegistration`],
//!    release the registry lock.
//! 2. Lock the event-store slot, clone the `Arc<dyn EventStore>`, release the
//!    slot lock.
//! 3. Call [`pull_page_over`] against the cloned store.
//! 4. **Encode the result AFTER the store transaction has ended.**
//!
//! No lock from an earlier step is held while a later step runs, and nothing is
//! dispatched/mutated under any lock. A null app or unknown cursor produces a
//! serialized `Error`, never a panic or null deref (D6).
//!
//! ## Result wire format (FFI-local, little-endian)
//!
//! The page/gap/error result is binary (it carries raw event JSON, which may
//! contain any byte), returned as owned [`NmpMirrorBytes`] the host frees with
//! [`nmp_mirror_free_bytes`]:
//!
//! ```text
//! result      := u8 variant
//!   variant 0 = Page  : u64 next_after_seq | u64 latest_seq | u8 has_more
//!                       | u32 entry_count | entry_count × entry
//!   variant 1 = Gap   : u64 requested_after_seq | u64 first_available_seq
//!   variant 2 = Error : u32 error_code  (see `error` consts below)
//! entry       := u64 seq | u8 op_tag (0=Inserted,1=Replaced,2=Deleted)
//!               | [Replaced] lp(replaced_id)
//!               | [Deleted]  lp(target_id) | u8 reason (0=Nip09,1=Nip40Expiry,2=AdminPurge)
//!               | lp(event_id) | u8 has_raw | [has_raw] lp(raw_json)
//!               | lp(source_relay)  (len 0 ⇒ absent) | u64 received_at_ms
//! lp(x)       := u32 byte_len | bytes        (length-prefixed UTF-8)
//! ```

use std::num::NonZeroUsize;

use nmp_core::{pull_page_over, PullCursorId, PullError, PullLimits};
use nmp_store::{EventStore, LogOp, ScanLogResult, StoreLogEntry};

use crate::NmpApp;

/// Hard ceiling on entries returned by one `pull_page` call (D5: bounded).
pub const MAX_PULL_PAGE_ENTRIES: u32 = 512;
/// Hard ceiling on cumulative raw-event bytes returned by one call (4 MiB).
pub const MAX_PULL_PAGE_RAW_BYTES: u32 = 4 * 1024 * 1024;

/// Serialized-result variant tags.
mod variant {
    pub const PAGE: u8 = 0;
    pub const GAP: u8 = 1;
    pub const ERROR: u8 = 2;
}

/// Serialized-result error codes (variant 2 payload).
pub mod error {
    /// The `app` pointer was null.
    pub const NULL_APP: u32 = 1;
    /// The cursor registry handle is unavailable (pre-start, or lock poisoned).
    pub const REGISTRY_UNAVAILABLE: u32 = 2;
    /// No cursor is registered under the requested id.
    pub const UNKNOWN_CURSOR: u32 = 3;
    /// The event store is unavailable (pre-start, or lock poisoned).
    pub const STORE_UNAVAILABLE: u32 = 4;
    /// The cursor scope could not be compiled to a store query
    /// (`PullError::UnsupportedInterestShape`).
    pub const UNSUPPORTED_SCOPE: u32 = 5;
    /// The underlying store returned an error (`PullError::Store`).
    pub const STORE_ERROR: u32 = 6;
    /// The cursor limits were logically invalid (`PullError::InvalidLimits`).
    pub const INVALID_LIMITS: u32 = 7;
    /// A panic was caught at the C-ABI boundary (never unwinds across FFI).
    pub const PANIC: u32 = 8;
    /// The first entry's raw event alone exceeds the raw-byte cap, so the page
    /// cannot be represented within the promised bound (D5: hard cap).
    pub const RAW_TOO_LARGE: u32 = 9;
}

/// op_tag wire values.
mod op {
    pub const INSERTED: u8 = 0;
    pub const REPLACED: u8 = 1;
    pub const DELETED: u8 = 2;
}

/// Owned heap buffer handed across the C-ABI for a mirror pull-page result.
///
/// The page/gap result is binary FlatBuffers-adjacent data that may contain NUL
/// bytes, so it cannot be a C string. The host MUST return this to Rust via
/// [`nmp_mirror_free_bytes`] exactly once; the buffer belongs to the Rust allocator.
///
/// Renamed from `NmpMirrorBytes` → `NmpMirrorBytes` (#1726) to gate raw history
/// behind the `nmp_mirror_*` family and make the ownership discipline explicit.
#[repr(C)]
pub struct NmpMirrorBytes {
    /// Pointer to `len` bytes (null only for the empty buffer).
    pub ptr: *mut u8,
    /// Number of valid bytes.
    pub len: usize,
    /// Allocation capacity (needed to reconstruct the `Vec` on free).
    pub cap: usize,
}

impl NmpMirrorBytes {
    fn from_vec(mut v: Vec<u8>) -> Self {
        let ptr = v.as_mut_ptr();
        let len = v.len();
        let cap = v.capacity();
        std::mem::forget(v);
        Self { ptr, len, cap }
    }

    fn error(code: u32) -> Self {
        let mut buf = Vec::with_capacity(5);
        buf.push(variant::ERROR);
        buf.extend_from_slice(&code.to_le_bytes());
        Self::from_vec(buf)
    }
}

/// Synchronously drain one page of the kernel ingest log for a registered cursor.
///
/// Returns serialized [`NmpMirrorBytes`] (Page / Gap / Error — see the module wire
/// format). `max_entries` is clamped to `[1, MAX_PULL_PAGE_ENTRIES]` and further
/// bounded by the cursor's registered `limits.max_entries`; cumulative raw bytes
/// are bounded by `min(max_total_raw_bytes, MAX_PULL_PAGE_RAW_BYTES)` (with at
/// least one entry always delivered so the cursor makes progress).
///
/// Null app / unknown cursor / unavailable store all return a serialized `Error`
/// — never a panic or null deref (D6).
#[no_mangle]
pub extern "C" fn nmp_mirror_pull_page(
    app: *const NmpApp,
    cursor_id: u64,
    max_entries: u32,
    max_total_raw_bytes: u32,
) -> NmpMirrorBytes {
    // A panic must NEVER unwind across the C-ABI (UB). Catch it and return a
    // serialized `Error::PANIC` instead (matches the FFI pattern in lib.rs).
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pull_page_impl(app, cursor_id, max_entries, max_total_raw_bytes)
    }))
    .unwrap_or_else(|_| NmpMirrorBytes::error(error::PANIC))
}

fn pull_page_impl(
    app: *const NmpApp,
    cursor_id: u64,
    max_entries: u32,
    max_total_raw_bytes: u32,
) -> NmpMirrorBytes {
    if app.is_null() {
        return NmpMirrorBytes::error(error::NULL_APP);
    }
    // SAFETY: `app` is non-null and, by C-ABI contract, a valid `NmpApp`
    // produced by `nmp_app_new`. This is a read-only borrow.
    let app = unsafe { &*app };

    // ── Step 1: snapshot the registration under the registry read-lock. ──────
    let registration = {
        let handle_slot = app.pull_cursor_registry_handle();
        let registry = {
            let Ok(guard) = handle_slot.lock() else {
                return NmpMirrorBytes::error(error::REGISTRY_UNAVAILABLE);
            };
            match guard.as_ref() {
                Some(reg) => reg.clone(),
                None => return NmpMirrorBytes::error(error::REGISTRY_UNAVAILABLE),
            }
        }; // outer Mutex released here
        let Ok(reg) = registry.read() else {
            return NmpMirrorBytes::error(error::REGISTRY_UNAVAILABLE);
        };
        match reg.get(&PullCursorId(cursor_id)) {
            Some(r) => r,
            None => return NmpMirrorBytes::error(error::UNKNOWN_CURSOR),
        }
    }; // registry read-lock released here

    // ── Step 2: clone the store handle under the event-store slot lock. ──────
    let store: std::sync::Arc<dyn EventStore> = {
        let slot = app.event_store_handle();
        let Ok(guard) = slot.lock() else {
            return NmpMirrorBytes::error(error::STORE_UNAVAILABLE);
        };
        match guard.as_ref() {
            Some(s) => std::sync::Arc::clone(s),
            None => return NmpMirrorBytes::error(error::STORE_UNAVAILABLE),
        }
    }; // event-store slot lock released here

    // Effective entry limit: clamp the request to the hard cap, then bound by
    // the cursor's registered limit. `NonZeroUsize` is guaranteed (clamp lower
    // bound is 1, and the registered max_entries is already non-zero).
    let clamped_entries = max_entries.clamp(1, MAX_PULL_PAGE_ENTRIES) as usize;
    let effective_entries = clamped_entries.min(registration.limits.max_entries.get());
    let effective_entries =
        NonZeroUsize::new(effective_entries).unwrap_or(registration.limits.max_entries);
    let limits = PullLimits {
        max_entries: effective_entries,
        max_scan_entries: registration.limits.max_scan_entries,
    };

    // ── Step 3: read the log against the cloned store (txn lives only here). ─
    let result = pull_page_over(
        store.as_ref(),
        registration.scope.clone(),
        registration.after_seq,
        limits,
    );

    // ── Step 4: encode AFTER the store transaction has ended. ────────────────
    let raw_byte_cap = max_total_raw_bytes.min(MAX_PULL_PAGE_RAW_BYTES) as usize;
    match result {
        Ok(ScanLogResult::Page(page)) => match encode_page(page, raw_byte_cap) {
            Ok(bytes) => NmpMirrorBytes::from_vec(bytes),
            Err(code) => NmpMirrorBytes::error(code),
        },
        Ok(ScanLogResult::Gap(gap)) => {
            NmpMirrorBytes::from_vec(encode_gap(gap.requested_after_seq, gap.first_available_seq))
        }
        Err(PullError::UnsupportedInterestShape) => NmpMirrorBytes::error(error::UNSUPPORTED_SCOPE),
        Err(PullError::InvalidLimits) => NmpMirrorBytes::error(error::INVALID_LIMITS),
        Err(PullError::Store(_)) => NmpMirrorBytes::error(error::STORE_ERROR),
    }
}

/// Release a buffer returned by [`nmp_mirror_pull_page`].
///
/// MUST be called exactly once for every returned [`NmpMirrorBytes`]. A null
/// pointer (the empty buffer) is a no-op (D6).
#[no_mangle]
pub extern "C" fn nmp_mirror_free_bytes(bytes: NmpMirrorBytes) {
    if bytes.ptr.is_null() {
        return;
    }
    // SAFETY: `bytes` was produced by `NmpMirrorBytes::from_vec` (ptr/len/cap of
    // a leaked `Vec<u8>`) and is freed exactly once per the ownership contract.
    unsafe {
        drop(Vec::from_raw_parts(bytes.ptr, bytes.len, bytes.cap));
    }
}

// ─── Encoding helpers ───────────────────────────────────────────────────────

fn put_lp(buf: &mut Vec<u8>, bytes: &[u8]) {
    #[allow(clippy::cast_possible_truncation)]
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
}

/// Length-prefixed lowercase-hex of a 32-byte id (64 ASCII chars), matching the
/// hex form the raw-event JSON carries.
fn put_hex32(buf: &mut Vec<u8>, id: &[u8; 32]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hex = [0u8; 64];
    for (i, b) in id.iter().enumerate() {
        hex[i * 2] = HEX[(b >> 4) as usize];
        hex[i * 2 + 1] = HEX[(b & 0x0f) as usize];
    }
    put_lp(buf, &hex);
}

fn encode_gap(requested_after_seq: u64, first_available_seq: u64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(17);
    buf.push(variant::GAP);
    buf.extend_from_slice(&requested_after_seq.to_le_bytes());
    buf.extend_from_slice(&first_available_seq.to_le_bytes());
    buf
}

/// Encode a `PullPage`, enforcing the cumulative raw-byte budget as a HARD cap.
///
/// Each raw event is serialized exactly ONCE (staged for re-use by
/// `encode_entry`). Subsequent rows that would push the cumulative raw bytes
/// past `raw_byte_cap` are dropped (`next_after_seq` rewinds to the last kept
/// seq so they redeliver next call). If the FIRST row's raw event alone exceeds
/// the cap it cannot be represented within the promised bound, so this returns
/// `Err(error::RAW_TOO_LARGE)` rather than silently overshooting (D5).
fn encode_page(page: nmp_store::PullPage, raw_byte_cap: usize) -> Result<Vec<u8>, u32> {
    let latest_seq = page.latest_seq;
    let store_next_after_seq = page.next_after_seq;
    let original_count = page.entries.len();

    // Stage (entry, pre-serialized raw json) so the raw is serialized once.
    let mut kept: Vec<(StoreLogEntry, Option<Vec<u8>>)> = Vec::new();
    let mut raw_total: usize = 0;
    for entry in page.entries {
        let json = entry
            .raw_event
            .as_ref()
            .and_then(|r| serde_json::to_vec(r).ok());
        let raw_len = json.as_ref().map_or(0, Vec::len);
        if kept.is_empty() {
            // First row: a raw event that alone overflows the cap cannot fit ⇒
            // explicit hard-cap error, never silently exceed the bound.
            if raw_len > raw_byte_cap {
                return Err(error::RAW_TOO_LARGE);
            }
        } else if raw_total.saturating_add(raw_len) > raw_byte_cap {
            break;
        }
        raw_total = raw_total.saturating_add(raw_len);
        kept.push((entry, json));
    }

    // Truncated by the byte budget ⇒ rewind the cursor to the last kept seq so
    // the dropped rows are redelivered next call. Untruncated ⇒ the store's
    // cursor stands (it already reflects the full page).
    let truncated = kept.len() < original_count;
    let next_after_seq = if truncated {
        kept.last().map_or(store_next_after_seq, |(e, _)| e.seq)
    } else {
        store_next_after_seq
    };
    let has_more = next_after_seq < latest_seq;

    let mut buf = Vec::with_capacity(21 + kept.len() * 96);
    buf.push(variant::PAGE);
    buf.extend_from_slice(&next_after_seq.to_le_bytes());
    buf.extend_from_slice(&latest_seq.to_le_bytes());
    buf.push(u8::from(has_more));
    #[allow(clippy::cast_possible_truncation)]
    buf.extend_from_slice(&(kept.len() as u32).to_le_bytes());
    for (entry, json) in &kept {
        encode_entry(&mut buf, entry, json.as_deref());
    }
    Ok(buf)
}

fn encode_entry(buf: &mut Vec<u8>, entry: &StoreLogEntry, raw_json: Option<&[u8]>) {
    buf.extend_from_slice(&entry.seq.to_le_bytes());
    match &entry.op {
        LogOp::Inserted => buf.push(op::INSERTED),
        LogOp::Replaced { replaced_id } => {
            buf.push(op::REPLACED);
            put_hex32(buf, replaced_id);
        }
        LogOp::Deleted { target_id, reason } => {
            buf.push(op::DELETED);
            put_hex32(buf, target_id);
            buf.push(encode_delete_reason(reason));
        }
    }
    put_hex32(buf, &entry.event_id);
    match raw_json {
        Some(json) => {
            buf.push(1);
            put_lp(buf, json);
        }
        None => buf.push(0),
    }
    match &entry.source_relay {
        Some(relay) => put_lp(buf, relay.as_bytes()),
        None => put_lp(buf, &[]),
    }
    buf.extend_from_slice(&entry.received_at_ms.to_le_bytes());
}

fn encode_delete_reason(reason: &nmp_store::DeleteReason) -> u8 {
    use nmp_store::DeleteReason;
    match reason {
        DeleteReason::Nip09 => 0,
        DeleteReason::Nip40Expiry => 1,
        DeleteReason::AdminPurge => 2,
    }
}

#[cfg(test)]
#[path = "pull_tests.rs"]
mod pull_tests;
