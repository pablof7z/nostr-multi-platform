//! Kernel-side identity / publish / relay-edit projection state.
//!
//! D0: these are wire-protocol projections (account = pubkey + npub +
//! signer-kind label, relay = url + role + status, publish-queue entry =
//! event id + kind + status). No app nouns leak; `nmp-signers` is NEVER
//! imported here (D0 forbids the `nmp-core -> nmp-signers` edge — the actor
//! adapts bare `nostr::Keys` and pushes these flat projections via the
//! setters below).
//!
//! D4: the actor thread is the single writer. These fields are a derived
//! cache of the actor's identity facts; the actor mutates them only through
//! `set_accounts` / `push_publish_entry` / `set_last_error_toast`, then emits.

use serde::{Deserialize, Serialize};

/// One account row in the snapshot. `signer_kind` is a stable label
/// (`"local"` | `"bunker"`) the UI renders verbatim — never a policy input.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct AccountSummary {
    /// Hex pubkey — the canonical `IdentityId` (matches NDK / applesauce).
    pub(crate) id: String,
    pub(crate) npub: String,
    pub(crate) display_name: String,
    pub(crate) signer_kind: String,
    /// `"active"` for the active account, `"idle"` otherwise.
    pub(crate) status: String,
}

/// One in-flight / recently-completed publish. Per D1 (best-effort
/// rendering) the UI shows the entry the moment it is enqueued; the status
/// refines in place as relay acks arrive.
///
/// Status lifecycle (T128 — terminal transitions):
/// - `"accepted_locally"` — engine has emitted EVENT frames; awaiting acks.
/// - `"ok"` — every required relay has terminally settled (at least one Ok,
///   no remaining FailedAfterRetries). Surfaces partial success too (Mixed
///   outcome → `"ok"` with per-relay detail in `relay_outcomes`).
/// - `"failed"` — every relay reached FailedAfterRetries (no Oks survived).
/// - Pre-T128 holdovers: `"pending_relays_unknown"` | `"duplicate"` |
///   `"store_error"`.
///
/// `relay_outcomes` carries the per-relay result map when the publish has
/// terminally settled; empty while still in flight or when the engine never
/// got that far (e.g. NoTargets). The iOS / Kotlin layers render this only
/// once `status` is terminal — they never read partial-state outcomes.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct PublishQueueEntry {
    pub(crate) event_id: String,
    pub(crate) kind: u32,
    pub(crate) target_relays: usize,
    pub(crate) status: String,
    /// Per-relay terminal outcomes, in insertion order. Empty while
    /// `status == "accepted_locally"` (no terminal verdict yet).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) relay_outcomes: Vec<RelayAckOutcome>,
}

/// One relay's terminal verdict for a publish. The string `status` keeps the
/// wire shape friendly to platforms that switch on tokens (Chirp's
/// `DiagnosticsView.statusColor` already does this for `"ok"`/`"failed"`).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct RelayAckOutcome {
    pub(crate) relay_url: String,
    /// `"ok"` for an accepted relay, `"failed"` for FailedAfterRetries.
    pub(crate) status: String,
    /// Empty for `"ok"`; carries the engine's give-up reason for `"failed"`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) message: String,
}

/// One relay row the UI's Accounts screen edits. Mirrors the kernel's
/// per-role `RelayHealth` for the relays Pulse drives.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct RelayEditRow {
    pub(crate) url: String,
    pub(crate) role: String,
}

/// NIP-46 bunker handshake progress projection. The broker (Stage 4) is the
/// sole writer of this field; the actor exposes it on the snapshot so the
/// SwiftUI sign-in flow can render handshake state ("connecting" →
/// "awaiting_pubkey" → "ready" or "failed"). `None` means no handshake is in
/// flight (the explicit `"idle"` stage from the broker maps to clearing).
///
/// Deserialize is included so Stage 2 (Swift codegen) can round-trip the type.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct BunkerHandshakeDto {
    /// `"connecting"` | `"awaiting_pubkey"` | `"ready"` | `"failed"` | `"idle"`
    /// (the wire never carries `"idle"`; the actor maps it to `None`).
    pub(crate) stage: String,
    /// Optional human-readable status (e.g. relay URL, error reason).
    pub(crate) message: Option<String>,
}

/// NIP-47 wallet connection status projected onto the snapshot.
/// Present when a wallet is (or was recently) connected; `None` when no
/// wallet has been connected in this session.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct WalletStatus {
    /// `"connecting"` | `"ready"` | `"error"` | `"disconnected"`
    pub(crate) status: String,
    /// The NWC relay URL (from the connection URI).
    pub(crate) relay_url: String,
    /// The wallet service pubkey in bech32 npub form.
    pub(crate) wallet_npub: String,
    /// Balance in millisatoshis, if the wallet has responded to `get_balance`.
    pub(crate) balance_msats: Option<u64>,
}

impl super::Kernel {
    /// Replace the account projection (D4: actor is sole writer).
    pub(crate) fn set_accounts(&mut self, accounts: Vec<AccountSummary>, active: Option<String>) {
        if self.accounts != accounts || self.active_account != active {
            self.accounts = accounts;
            self.active_account = active;
            self.changed_since_emit = true;
        }
    }

    /// Append a publish-queue entry, keeping a bounded recent window (D5).
    pub(crate) fn push_publish_entry(&mut self, entry: PublishQueueEntry) {
        self.publish_queue.push(entry);
        // Bounded recent window — D5 (snapshots bounded by what's open).
        const MAX_PUBLISH_WINDOW: usize = 16;
        if self.publish_queue.len() > MAX_PUBLISH_WINDOW {
            let drop = self.publish_queue.len() - MAX_PUBLISH_WINDOW;
            self.publish_queue.drain(0..drop);
        }
        self.changed_since_emit = true;
    }

    /// Patch the queue entry for `event_id` in place, flipping `status` and
    /// recording the per-relay outcome map. T128 — D1 (refine in place); the
    /// entry was originally pushed as `accepted_locally`, and the engine's
    /// terminal observation now refines it. No-op if no row matches
    /// (defensive — the bounded 16-row window may have already evicted it).
    pub(crate) fn set_publish_entry_terminal(
        &mut self,
        event_id: &str,
        status: &str,
        outcomes: Vec<RelayAckOutcome>,
    ) {
        let Some(entry) = self
            .publish_queue
            .iter_mut()
            .rev() // most recent first — handles the common case fast
            .find(|e| e.event_id == event_id)
        else {
            return;
        };
        if entry.status == status && entry.relay_outcomes == outcomes {
            return;
        }
        entry.status = status.to_string();
        entry.relay_outcomes = outcomes;
        self.changed_since_emit = true;
    }

    /// Surface a coarse error string to the UI (D6: errors are state, never
    /// exceptions across FFI). `None` clears the toast.
    pub(crate) fn set_last_error_toast(&mut self, toast: Option<String>) {
        if self.last_error_toast != toast {
            self.last_error_toast = toast;
            self.changed_since_emit = true;
        }
    }

    /// Replace the editable relay projection (D4: actor is sole writer).
    pub(crate) fn set_relay_edit_rows(&mut self, rows: Vec<RelayEditRow>) {
        if self.relay_edit_rows != rows {
            self.relay_edit_rows = rows;
            self.changed_since_emit = true;
        }
    }

    /// Replace the wallet status projection (D4: actor is sole writer).
    pub(crate) fn set_wallet_status(&mut self, status: Option<WalletStatus>) {
        if self.wallet_status != status {
            self.wallet_status = status;
            self.changed_since_emit = true;
        }
    }

    /// Replace the NIP-46 bunker handshake projection. Stage 3 of NIP-46 wiring:
    /// the broker (Stage 4) is the sole driver; the actor's `sign_in_bunker`
    /// command also seeds the initial `"connecting"` value on shape-valid URIs.
    /// Pass `None` (or stage `"idle"`) to clear.
    pub(crate) fn set_bunker_handshake(&mut self, value: Option<BunkerHandshakeDto>) {
        if self.bunker_handshake != value {
            self.bunker_handshake = value;
            self.changed_since_emit = true;
        }
    }

    pub(crate) fn account_snapshot(&self) -> (&[AccountSummary], Option<&String>) {
        (&self.accounts, self.active_account.as_ref())
    }

    pub(crate) fn publish_queue_snapshot(&self) -> &[PublishQueueEntry] {
        &self.publish_queue
    }

    pub(crate) fn last_error_toast_snapshot(&self) -> Option<&String> {
        self.last_error_toast.as_ref()
    }

    pub(crate) fn relay_edit_rows_snapshot(&self) -> &[RelayEditRow] {
        &self.relay_edit_rows
    }

    pub(crate) fn wallet_status_snapshot(&self) -> Option<&WalletStatus> {
        self.wallet_status.as_ref()
    }

    pub(crate) fn bunker_handshake_snapshot(&self) -> Option<&BunkerHandshakeDto> {
        self.bunker_handshake.as_ref()
    }
}
