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

use serde::Serialize;

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
/// refines in place as relay acks arrive. Per the T66a scope, the entry is
/// marked `accepted_locally` the moment EVENT frames are emitted — full
/// per-relay OK correlation is a follow-up (D1: refine in place).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct PublishQueueEntry {
    pub(crate) event_id: String,
    pub(crate) kind: u32,
    pub(crate) target_relays: usize,
    /// `"accepted_locally"` | `"pending_relays_unknown"` | `"duplicate"` |
    /// `"store_error"`.
    pub(crate) status: String,
}

/// One relay row the UI's Accounts screen edits. Mirrors the kernel's
/// per-role `RelayHealth` for the relays Pulse drives.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct RelayEditRow {
    pub(crate) url: String,
    pub(crate) role: String,
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
}
