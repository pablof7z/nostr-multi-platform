//! ADR-0058 §3 (step 3b) — synchronous, read-only FFI pull-page surface.
//!
//! [`nmp_app_pull_page`] is the C-ABI door a host (a nostrdb mirror, or a feed's
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
//! contain any byte), returned as owned [`NmpOwnedBytes`] the host frees with
//! [`nmp_free_bytes`]:
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

use nmp_core::store::{EventStore, LogOp, ScanLogResult, StoreLogEntry};
use nmp_core::{pull_page_over, PullCursorId, PullError, PullLimits};

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
}

/// op_tag wire values.
mod op {
    pub const INSERTED: u8 = 0;
    pub const REPLACED: u8 = 1;
    pub const DELETED: u8 = 2;
}

/// Owned heap buffer handed across the C-ABI for a pull-page result.
///
/// The page/gap result is binary FlatBuffers-adjacent data that may contain NUL
/// bytes, so it cannot be a C string. The host MUST return this to Rust via
/// [`nmp_free_bytes`] exactly once; the buffer belongs to the Rust allocator.
#[repr(C)]
pub struct NmpOwnedBytes {
    /// Pointer to `len` bytes (null only for the empty buffer).
    pub ptr: *mut u8,
    /// Number of valid bytes.
    pub len: usize,
    /// Allocation capacity (needed to reconstruct the `Vec` on free).
    pub cap: usize,
}

impl NmpOwnedBytes {
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
/// Returns serialized [`NmpOwnedBytes`] (Page / Gap / Error — see the module wire
/// format). `max_entries` is clamped to `[1, MAX_PULL_PAGE_ENTRIES]` and further
/// bounded by the cursor's registered `limits.max_entries`; cumulative raw bytes
/// are bounded by `min(max_total_raw_bytes, MAX_PULL_PAGE_RAW_BYTES)` (with at
/// least one entry always delivered so the cursor makes progress).
///
/// Null app / unknown cursor / unavailable store all return a serialized `Error`
/// — never a panic or null deref (D6).
#[no_mangle]
pub extern "C" fn nmp_app_pull_page(
    app: *const NmpApp,
    cursor_id: u64,
    max_entries: u32,
    max_total_raw_bytes: u32,
) -> NmpOwnedBytes {
    if app.is_null() {
        return NmpOwnedBytes::error(error::NULL_APP);
    }
    // SAFETY: `app` is non-null and, by C-ABI contract, a valid `NmpApp`
    // produced by `nmp_app_new`. This is a read-only borrow.
    let app = unsafe { &*app };

    // ── Step 1: snapshot the registration under the registry read-lock. ──────
    let registration = {
        let handle_slot = app.pull_cursor_registry_handle();
        let registry = {
            let Ok(guard) = handle_slot.lock() else {
                return NmpOwnedBytes::error(error::REGISTRY_UNAVAILABLE);
            };
            match guard.as_ref() {
                Some(reg) => reg.clone(),
                None => return NmpOwnedBytes::error(error::REGISTRY_UNAVAILABLE),
            }
        }; // outer Mutex released here
        let Ok(reg) = registry.read() else {
            return NmpOwnedBytes::error(error::REGISTRY_UNAVAILABLE);
        };
        match reg.get(&PullCursorId(cursor_id)) {
            Some(r) => r,
            None => return NmpOwnedBytes::error(error::UNKNOWN_CURSOR),
        }
    }; // registry read-lock released here

    // ── Step 2: clone the store handle under the event-store slot lock. ──────
    let store: std::sync::Arc<dyn EventStore> = {
        let slot = app.event_store_handle();
        let Ok(guard) = slot.lock() else {
            return NmpOwnedBytes::error(error::STORE_UNAVAILABLE);
        };
        match guard.as_ref() {
            Some(s) => std::sync::Arc::clone(s),
            None => return NmpOwnedBytes::error(error::STORE_UNAVAILABLE),
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
    let bytes = match result {
        Ok(ScanLogResult::Page(page)) => encode_page(page, raw_byte_cap),
        Ok(ScanLogResult::Gap(gap)) => encode_gap(gap.requested_after_seq, gap.first_available_seq),
        Err(PullError::UnsupportedInterestShape) => {
            return NmpOwnedBytes::error(error::UNSUPPORTED_SCOPE)
        }
        Err(PullError::InvalidLimits) => return NmpOwnedBytes::error(error::INVALID_LIMITS),
        Err(PullError::Store(_)) => return NmpOwnedBytes::error(error::STORE_ERROR),
    };
    NmpOwnedBytes::from_vec(bytes)
}

/// Release a buffer returned by [`nmp_app_pull_page`].
///
/// MUST be called exactly once for every returned [`NmpOwnedBytes`]. A null
/// pointer (the empty buffer) is a no-op (D6).
#[no_mangle]
pub extern "C" fn nmp_free_bytes(bytes: NmpOwnedBytes) {
    if bytes.ptr.is_null() {
        return;
    }
    // SAFETY: `bytes` was produced by `NmpOwnedBytes::from_vec` (ptr/len/cap of
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

/// Encode a `PullPage`, enforcing the cumulative raw-byte budget. At least one
/// entry is always kept (so an oversized single event still makes progress);
/// truncation rewrites `next_after_seq` to the last kept entry's seq and
/// recomputes `has_more`.
fn encode_page(page: nmp_core::store::PullPage, raw_byte_cap: usize) -> Vec<u8> {
    let latest_seq = page.latest_seq;
    let store_next_after_seq = page.next_after_seq;
    let original_count = page.entries.len();

    let mut kept: Vec<StoreLogEntry> = Vec::new();
    let mut raw_total: usize = 0;
    for entry in page.entries {
        let raw_len = entry
            .raw_event
            .as_ref()
            .and_then(|r| serde_json::to_vec(r).ok())
            .map_or(0, |j| j.len());
        if !kept.is_empty() && raw_total.saturating_add(raw_len) > raw_byte_cap {
            break;
        }
        raw_total = raw_total.saturating_add(raw_len);
        kept.push(entry);
    }

    // Truncated by the byte budget ⇒ rewind the cursor to the last kept seq so
    // the dropped rows are redelivered next call. Untruncated ⇒ the store's
    // cursor stands (it already reflects the full page).
    let truncated = kept.len() < original_count;
    let next_after_seq = if truncated {
        kept.last().map_or(store_next_after_seq, |e| e.seq)
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
    for entry in &kept {
        encode_entry(&mut buf, entry);
    }
    buf
}

fn encode_entry(buf: &mut Vec<u8>, entry: &StoreLogEntry) {
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
    match entry.raw_event.as_ref().and_then(|r| serde_json::to_vec(r).ok()) {
        Some(json) => {
            buf.push(1);
            put_lp(buf, &json);
        }
        None => buf.push(0),
    }
    match &entry.source_relay {
        Some(relay) => put_lp(buf, relay.as_bytes()),
        None => put_lp(buf, &[]),
    }
    buf.extend_from_slice(&entry.received_at_ms.to_le_bytes());
}

fn encode_delete_reason(reason: &nmp_core::store::DeleteReason) -> u8 {
    use nmp_core::store::DeleteReason;
    match reason {
        DeleteReason::Nip09 => 0,
        DeleteReason::Nip40Expiry => 1,
        DeleteReason::AdminPurge => 2,
    }
}

#[cfg(test)]
#[path = "pull_tests.rs"]
mod pull_tests;
