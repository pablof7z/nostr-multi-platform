//! Log-safe runtime diagnostics for the browser runtime (#2075).
//!
//! `BrowserRuntimeDiagnostics` is a redacted snapshot of the runtime's
//! current state, safe to log, serialize, and expose to JS hosts without
//! leaking secret material (no nsec / hex keys / DM content / error bodies).
//!
//! # Redaction rules (enforced in the constructor)
//!
//! - **No secret material**: no nsec, no private keys, no tokens.
//! - **Identity is prefix-only**: `active_account_npub_prefix` = first 8 chars
//!   of the bech32 `npub`, never the full key or hex.
//! - **Errors are category, not body**: `last_error_category` is the error
//!   category tag from the kernel snapshot, NOT the toast/planner-error text.
//! - **Counts, not contents**: relay count, pending sign count — never the
//!   relay URLs or event content.
//! - **D6 — total**: a poisoned signer-state slot, or any field that panics,
//!   degrades to the `Default` value for that field. The whole struct defaults
//!   on any unrecoverable error.

use nmp_core::{decode_snapshot_envelope, SignerStateModel};
use serde::Serialize;

use super::signer_state::BrowserSignerStateSlot;

/// A log-safe, redacted view of the browser runtime's current state.
///
/// Built by [`BrowserRuntimeDiagnostics::build`] from the last good merged
/// frame + configured relay count + pending sign count + signer-state slot.
///
/// All fields are safe to log (no secret material, no DM/event content,
/// identity is prefix-only). See module-level doc for full redaction rules.
#[derive(Debug, Clone, Default, Serialize)]
pub struct BrowserRuntimeDiagnostics {
    /// Whether the kernel is in the running state.
    pub running: bool,
    /// Monotonically-increasing frame revision.
    pub snapshot_rev: u64,
    /// Within-session epoch counter (bumped on account-switch/resync).
    pub snapshot_epoch: u64,
    /// Kernel-start wall-clock ms (changes across process restarts).
    pub session_id: u64,
    /// Current actor command-queue depth (backpressure metric).
    pub actor_queue_depth: u32,
    /// Number of publishes parked awaiting a signature.
    pub pending_sign_count: usize,
    /// Number of configured relay URLs.
    pub configured_relay_count: usize,
    /// Error category from the kernel snapshot (NOT the full toast/body text).
    /// `None` when no error category is set.
    pub last_error_category: Option<String>,
    /// Signer kind string (e.g. `"nip46"`, `"nip55"`, `"local"`).
    /// `None` when no signer is active.
    pub signer_kind: Option<String>,
    /// Signer state string (e.g. `"ready"`, `"reconnecting"`, `"failed"`).
    /// `None` when no signer is active.
    pub signer_state: Option<String>,
    /// First 8 characters of the active account's bech32 `npub` (prefix only
    /// — never the full npub, hex pubkey, or nsec). `None` when no account.
    pub active_account_npub_prefix: Option<String>,
}

impl BrowserRuntimeDiagnostics {
    /// Build a [`BrowserRuntimeDiagnostics`] from the given runtime state.
    ///
    /// `merged_frame`: the last successfully merged snapshot frame bytes
    ///   (from `BrowserSnapshotCache::last_good`). `None` → all envelope
    ///   fields default to zero/None.
    ///
    /// On any decode error or poisoned lock, the affected field defaults
    /// (D6 — total). The `active_account_npub_prefix` uses nmp-core's
    /// `display` helpers to produce a safe prefix without leaking the key.
    pub fn build(
        merged_frame: Option<&[u8]>,
        pending_sign_count: usize,
        configured_relay_count: usize,
        signer_state_slot: &BrowserSignerStateSlot,
        active_account_pubkey_hex: Option<String>,
    ) -> Self {
        // ── Decode the snapshot envelope (Tier-3 fields) ─────────────────────
        let envelope = merged_frame.and_then(|bytes| decode_snapshot_envelope(bytes).ok());

        let (
            running,
            snapshot_rev,
            snapshot_epoch,
            session_id,
            actor_queue_depth,
            last_error_category,
        ) = envelope
            .map(|e| {
                (
                    e.running,
                    e.rev,
                    e.snapshot_epoch,
                    e.session_id,
                    e.actor_queue_depth,
                    e.last_error_category, // category only, never toast body
                )
            })
            .unwrap_or_default();

        // ── Signer state (poisoned → None, D6) ───────────────────────────────
        let (signer_kind, signer_state_str) = read_signer_state(signer_state_slot);

        // ── Identity prefix (8 chars max, never full key) ────────────────────
        let active_account_npub_prefix =
            active_account_pubkey_hex.and_then(|hex| npub_prefix_8(&hex));

        Self {
            running,
            snapshot_rev,
            snapshot_epoch,
            session_id,
            actor_queue_depth,
            pending_sign_count,
            configured_relay_count,
            last_error_category,
            signer_kind,
            signer_state: signer_state_str,
            active_account_npub_prefix,
        }
    }

    /// Serialize this diagnostics snapshot to a compact JSON string.
    ///
    /// D6 — total: on serialisation error returns a minimal JSON error marker
    /// rather than panicking. No secret material escapes (the struct itself
    /// only carries redacted fields).
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error":"diagnostics_serialize_failed"}"#.to_string())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Read `(signer_kind, signer_state)` from the slot.
///
/// D6: a poisoned lock yields `(None, None)` — we do NOT recover and serialize
/// the poisoned value (recovered-poisoned data must never be presented; the
/// contract is poison → defaults / no sidecar). An idle slot is `(None, None)`.
fn read_signer_state(slot: &BrowserSignerStateSlot) -> (Option<String>, Option<String>) {
    let Ok(guard) = slot.lock() else {
        return (None, None);
    };
    match guard.model() {
        None => (None, None),
        Some(SignerStateModel {
            signer_kind, state, ..
        }) => (Some(signer_kind.clone()), Some(state.clone())),
    }
}

/// Derive the first 8 bech32 characters of an npub from a hex pubkey string.
///
/// Encodes the raw 32-byte pubkey as bech32 `npub` then returns the first
/// 8 characters (e.g. `"npub1abc"`). This is enough to identify an account
/// in logs without revealing the full key. Returns `None` on any error
/// (bad hex, invalid pubkey length, bech32 failure).
fn npub_prefix_8(hex_pubkey: &str) -> Option<String> {
    use nmp_core::nip19::encode_npub;
    // Use nip19 to encode to npub, then take prefix.
    let npub = encode_npub(hex_pubkey).ok()?;
    // Take first 8 chars (e.g. "npub1abc") — never the full key.
    let prefix: String = npub.chars().take(8).collect();
    if prefix.is_empty() {
        None
    } else {
        Some(prefix)
    }
}
