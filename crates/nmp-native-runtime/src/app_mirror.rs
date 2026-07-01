//! ADR-0058 §3 mirror pull-page core — runtime-side encoding and pull logic.
//!
//! Lives here (not in `nmp-uniffi`) so the UniFFI surface
//! (`nmp_uniffi::mirror::NmpApp::mirror_pull_page`) and any internal tests can
//! use the SAME underlying implementation without duplication.
//!
//! ## Key contract: single runtime implementation
//!
//! `nmp-uniffi/src/mirror.rs` calls straight into these constants and helpers;
//! it is the only native binding surface (the old `nmp-ffi` C-ABI crate and its
//! binary entry section were deleted). Hosts call the typed UniFFI mirror pull
//! surface.
//!
//! ## Wire format (little-endian)
//!
//! ```text
//! result      := u8 variant
//!   variant 0 = Page  : u64 next_after_seq | u64 latest_seq | u8 has_more
//!                       | u32 entry_count | entry_count × entry
//!   variant 1 = Gap   : u64 requested_after_seq | u64 first_available_seq
//!   variant 2 = Error : u32 error_code  (see `error` module below)
//! entry       := u64 seq | u8 op_tag (0=Inserted,1=Replaced,2=Deleted)
//!               | [Replaced] lp(replaced_id)
//!               | [Deleted]  lp(target_id) | u8 reason (0=Nip09,1=Nip40Expiry,2=AdminPurge)
//!               | lp(event_id) | u8 has_raw | [has_raw] lp(raw_json)
//!               | lp(source_relay)  (len 0 ⇒ absent) | u64 received_at_ms
//! lp(x)       := u32 byte_len | bytes
//! ```

use std::num::NonZeroUsize;
use std::sync::Arc;

use nmp_core::{pull_page_over, PullCursorId, PullError, PullLimits};
use nmp_store::{DeleteReason, EventStore, LogOp, PullPage, ScanLogResult, StoreLogEntry};

use crate::app_struct::NmpApp;

// ── Public constants ──────────────────────────────────────────────────────────

/// Hard ceiling on entries returned by one `mirror_pull_page` call (D5: bounded).
pub const MAX_PULL_PAGE_ENTRIES: u32 = 512;

/// Hard ceiling on cumulative raw-event bytes returned by one call (4 MiB).
pub const MAX_PULL_PAGE_RAW_BYTES: u32 = 4 * 1024 * 1024;

// ── Variant tags ──────────────────────────────────────────────────────────────

/// Wire-format variant byte values.
///
/// `pub` so callers can share the same runtime encoding constants.
pub mod variant {
    pub const PAGE: u8 = 0;
    pub const GAP: u8 = 1;
    pub const ERROR: u8 = 2;
}

// ── Error codes ───────────────────────────────────────────────────────────────

/// Serialized-result error codes (variant 2 payload).
///
/// `pub` so `nmp-uniffi/src/mirror.rs` can reference codes such as
/// `error::PANIC` directly.
pub mod error {
    /// Reserved for a null-app condition from the old C-ABI pointer surface
    /// (deleted); not reachable via UniFFI, which never passes a raw pointer.
    pub const NULL_APP: u32 = 1;
    /// The cursor registry handle is unavailable (pre-start, or lock poisoned).
    pub const REGISTRY_UNAVAILABLE: u32 = 2;
    /// No cursor is registered under the requested id.
    pub const UNKNOWN_CURSOR: u32 = 3;
    /// The event store is unavailable (pre-start, or lock poisoned).
    pub const STORE_UNAVAILABLE: u32 = 4;
    /// The cursor scope could not be compiled to a store query.
    pub const UNSUPPORTED_SCOPE: u32 = 5;
    /// The underlying store returned an error (`PullError::Store`).
    pub const STORE_ERROR: u32 = 6;
    /// The cursor limits were logically invalid (`PullError::InvalidLimits`).
    pub const INVALID_LIMITS: u32 = 7;
    /// A panic was caught at the ABI boundary.
    pub const PANIC: u32 = 8;
    /// The first entry's raw event alone exceeds the raw-byte cap (D5: hard cap).
    pub const RAW_TOO_LARGE: u32 = 9;
}

// ── op_tag wire values ────────────────────────────────────────────────────────

mod op {
    pub const INSERTED: u8 = 0;
    pub const REPLACED: u8 = 1;
    pub const DELETED: u8 = 2;
}

// ── Encoding helpers ──────────────────────────────────────────────────────────
//
// `pub` so `nmp-uniffi/src/mirror.rs` and this module's own tests can call
// them directly (`encode_gap`, `encode_page`).

/// Build the 5-byte error payload for `variant = ERROR`.
pub fn error_bytes(code: u32) -> Vec<u8> {
    let mut buf = Vec::with_capacity(5);
    buf.push(variant::ERROR);
    buf.extend_from_slice(&code.to_le_bytes());
    buf
}

/// Append a length-prefixed byte slice (u32 LE length + bytes).
pub fn put_lp(buf: &mut Vec<u8>, bytes: &[u8]) {
    #[allow(clippy::cast_possible_truncation)]
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
}

/// Append a length-prefixed lowercase-hex encoding of a 32-byte id (64 ASCII chars).
pub fn put_hex32(buf: &mut Vec<u8>, id: &[u8; 32]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hex = [0u8; 64];
    for (i, b) in id.iter().enumerate() {
        hex[i * 2] = HEX[(b >> 4) as usize];
        hex[i * 2 + 1] = HEX[(b & 0x0f) as usize];
    }
    put_lp(buf, &hex);
}

/// Map a [`DeleteReason`] to its wire-format reason byte.
pub fn encode_delete_reason(reason: &DeleteReason) -> u8 {
    match reason {
        DeleteReason::Nip09 => 0,
        DeleteReason::Nip40Expiry => 1,
        DeleteReason::AdminPurge => 2,
    }
}

/// Append a single [`StoreLogEntry`] to `buf`.
pub fn encode_entry(buf: &mut Vec<u8>, entry: &StoreLogEntry, raw_json: Option<&[u8]>) {
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

/// Encode a Gap result.
pub fn encode_gap(requested_after_seq: u64, first_available_seq: u64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(17);
    buf.push(variant::GAP);
    buf.extend_from_slice(&requested_after_seq.to_le_bytes());
    buf.extend_from_slice(&first_available_seq.to_le_bytes());
    buf
}

/// Encode a [`PullPage`], enforcing the cumulative raw-byte budget as a hard cap.
///
/// Returns `Err(error::RAW_TOO_LARGE)` if the first row's raw event alone
/// exceeds `raw_byte_cap` (D5: cannot represent within the promised bound).
/// Subsequent rows that would push past the cap are dropped; `next_after_seq`
/// rewinds to the last kept seq so they redeliver on the next call.
pub fn encode_page(page: PullPage, raw_byte_cap: usize) -> Result<Vec<u8>, u32> {
    let latest_seq = page.latest_seq;
    let store_next_after_seq = page.next_after_seq;
    let original_count = page.entries.len();

    // Stage (entry, pre-serialized raw json) so each raw is serialized once.
    let mut kept: Vec<(StoreLogEntry, Option<Vec<u8>>)> = Vec::new();
    let mut raw_total: usize = 0;
    for entry in page.entries {
        let json = entry
            .raw_event
            .as_ref()
            .and_then(|r| serde_json::to_vec(r).ok());
        let raw_len = json.as_ref().map_or(0, Vec::len);
        if kept.is_empty() {
            // First row: a raw event that alone overflows the cap cannot fit.
            if raw_len > raw_byte_cap {
                return Err(error::RAW_TOO_LARGE);
            }
        } else if raw_total.saturating_add(raw_len) > raw_byte_cap {
            break;
        }
        raw_total = raw_total.saturating_add(raw_len);
        kept.push((entry, json));
    }

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

// ── NmpApp::mirror_pull_page_raw_bytes ────────────────────────────────────────

impl NmpApp {
    /// Synchronously drain one page of the kernel ingest log for a registered
    /// cursor, returning the result as a serialized binary payload.
    ///
    /// This is the single implementation behind the UniFFI surface
    /// (`NmpApp::mirror_pull_page` in `nmp-uniffi/src/mirror.rs`). The UniFFI
    /// layer lifts the typed header fields out and forwards the entry bytes.
    ///
    /// ## Parameters
    ///
    /// - `cursor_id` — raw u64 cursor id minted by
    ///   `PullCursorRegistry::alloc_handle`.
    /// - `max_entries` — clamped to `[1, MAX_PULL_PAGE_ENTRIES]`; further
    ///   bounded by the cursor's registered `limits.max_entries`.
    /// - `raw_byte_cap` — cumulative raw-event byte budget; capped internally
    ///   at `MAX_PULL_PAGE_RAW_BYTES`. At least one entry is always delivered.
    ///
    /// ## Return value
    ///
    /// The returned `Vec<u8>` follows the wire format in the module doc:
    /// variant byte + payload. Errors (registry unavailable, unknown cursor,
    /// store unavailable, pull failure) encode as `variant=ERROR | u32 code`
    /// — never a panic (D6).
    ///
    /// ## Lock order (ADR-0058 §3)
    ///
    /// 1. Read-lock registry → clone registration → release.
    /// 2. Lock event-store slot → clone Arc → release.
    /// 3. Call `pull_page_over` (store txn, no lock held from steps 1-2).
    /// 4. Encode AFTER the store transaction has ended.
    #[must_use]
    pub fn mirror_pull_page_raw_bytes(
        &self,
        cursor_id: u64,
        max_entries: u32,
        raw_byte_cap: usize,
    ) -> Vec<u8> {
        // ── Step 1: snapshot the registration under the registry read-lock. ──
        let registration = {
            let handle_slot = self.pull_cursor_registry_handle();
            let registry = {
                let Ok(guard) = handle_slot.lock() else {
                    return error_bytes(error::REGISTRY_UNAVAILABLE);
                };
                match guard.as_ref() {
                    Some(reg) => reg.clone(),
                    None => return error_bytes(error::REGISTRY_UNAVAILABLE),
                }
            }; // outer Mutex released here
            let Ok(reg) = registry.read() else {
                return error_bytes(error::REGISTRY_UNAVAILABLE);
            };
            match reg.get(&PullCursorId(cursor_id)) {
                Some(r) => r,
                None => return error_bytes(error::UNKNOWN_CURSOR),
            }
        }; // registry read-lock released here

        // ── Step 2: clone the store handle under the event-store slot lock. ──
        let store: Arc<dyn EventStore> = {
            let slot = self.event_store_handle();
            let Ok(guard) = slot.lock() else {
                return error_bytes(error::STORE_UNAVAILABLE);
            };
            match guard.as_ref() {
                Some(s) => Arc::clone(s),
                None => return error_bytes(error::STORE_UNAVAILABLE),
            }
        }; // event-store slot lock released here

        // Effective entry limit: clamp to hard cap, then bound by cursor's limit.
        let clamped_entries = max_entries.clamp(1, MAX_PULL_PAGE_ENTRIES) as usize;
        let effective_entries = clamped_entries.min(registration.limits.max_entries.get());
        let effective_entries =
            NonZeroUsize::new(effective_entries).unwrap_or(registration.limits.max_entries);
        let limits = PullLimits {
            max_entries: effective_entries,
            max_scan_entries: registration.limits.max_scan_entries,
        };

        // ── Step 3: read the log (txn lives only inside pull_page_over). ─────
        let result = pull_page_over(
            store.as_ref(),
            registration.scope.clone(),
            registration.after_seq,
            limits,
        );

        // ── Step 4: encode AFTER the store transaction has ended. ────────────
        let effective_cap = raw_byte_cap.min(MAX_PULL_PAGE_RAW_BYTES as usize);
        match result {
            Ok(ScanLogResult::Page(page)) => match encode_page(page, effective_cap) {
                Ok(bytes) => bytes,
                Err(code) => error_bytes(code),
            },
            Ok(ScanLogResult::Gap(gap)) => {
                encode_gap(gap.requested_after_seq, gap.first_available_seq)
            }
            Err(PullError::UnsupportedInterestShape) => error_bytes(error::UNSUPPORTED_SCOPE),
            Err(PullError::InvalidLimits) => error_bytes(error::INVALID_LIMITS),
            Err(PullError::Store(_)) => error_bytes(error::STORE_ERROR),
        }
    }
}
