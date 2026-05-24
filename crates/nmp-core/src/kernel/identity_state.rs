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

use std::sync::{Arc, Mutex};

use serde::Serialize;

/// Shared slot for the currently active account pubkey.
///
/// Follows the same typed-slot pattern as [`IndexerRelaysSlot`] and
/// [`LocalWriteRelaysSlot`] in `relay_projection`: a named type alias prevents
/// accidental bare `Arc<Mutex<Option<String>>>` proliferation and lets D14's
/// lint catch shape regressions at the declaration site rather than silently at
/// every call site.
pub(crate) type ActiveAccountSlot = Arc<Mutex<Option<String>>>;

/// Construct a fresh, empty [`ActiveAccountSlot`].
pub(crate) fn new_active_account_slot() -> ActiveAccountSlot {
    Arc::new(Mutex::new(None))
}

/// One account row in the snapshot.
///
/// `signer_kind` is the stable wire token (`"local"` | `"nip46"` | …) other
/// platforms switch on; it is kept for backward compatibility with Android +
/// diagnostic surfaces, but Swift no longer derives display labels from it
/// (aim.md §4.4 / §4.5). Native should bind the pre-classified fields below.
///
/// Pre-classified fields (D4: actor populates, Swift binds):
/// - `signer_label` — human-readable label for the row's signer.
/// - `signer_is_remote` — `true` for any signer whose key material lives
///   outside the kernel (NIP-46 today, NIP-07 / hardware later). Lets the UI
///   scope a "remote signers" section without lowercased string filtering.
/// - `is_active` — pre-derived `status == "active"` so view code does not
///   compare strings to decide active-ness. `status` stays for the same
///   backward-compat reason as `signer_kind`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "codegen-schema", derive(schemars::JsonSchema))]
pub(crate) struct AccountSummary {
    /// Hex pubkey — the canonical `IdentityId` (matches NDK / applesauce).
    pub(crate) id: String,
    pub(crate) npub: String,
    /// Pre-formatted abbreviated bech32 npub (`npub1abcd…xyz`). Computed at
    /// construction time so Swift never abbreviates npubs in-view (thin-shell
    /// V-24; same pattern as `ProfileCard.npub_short`). The algorithm
    /// matches `profile_npub_short` in `kernel/update.rs` byte-for-byte —
    /// deliberate micro-duplication keeps the kernel modules decoupled
    /// (`identity_state.rs` does not reach across into `update.rs`'s
    /// private helpers).
    pub(crate) npub_short: String,
    pub(crate) display_name: String,
    pub(crate) signer_kind: String,
    /// `"active"` for the active account, `"idle"` otherwise.
    pub(crate) status: String,
    /// Pre-classified, human-readable signer label (e.g. `"nsec"`, `"NIP-46"`).
    /// Swift renders this verbatim instead of switching on `signer_kind`.
    pub(crate) signer_label: String,
    /// `true` when the signer's key material lives outside the kernel
    /// (NIP-46 bunker today, NIP-07 / hardware later). Lets native scope
    /// remote-signer-only sections without string-matching `signer_kind`.
    pub(crate) signer_is_remote: bool,
    /// Pre-derived `status == "active"`. Native binds this directly.
    pub(crate) is_active: bool,
    /// Profile picture URL from kind:0 metadata. `None` when no kind:0 has
    /// been received yet; enriched by `Kernel::accounts_enriched()` in
    /// the snapshot builder so the toolbar avatar shows the user's real picture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) picture_url: Option<String>,
}

/// One in-flight / recently-completed publish. Per D1 (best-effort
/// rendering) the UI shows the entry the moment it is enqueued; the status
/// refines in place as relay acks arrive.
///
/// Status lifecycle (T128 — terminal transitions):
/// - `"accepted_locally"` — engine has emitted EVENT frames; awaiting acks.
/// - `"ok"` — every required relay has terminally settled (at least one Ok,
///   no remaining `FailedAfterRetries`). Surfaces partial success too (Mixed
///   outcome → `"ok"` with per-relay detail in `relay_outcomes`).
/// - `"failed"` — every relay reached `FailedAfterRetries` (no Oks survived).
/// - Pre-T128 holdovers: `"pending_relays_unknown"` | `"duplicate"` |
///   `"store_error"`.
///
/// `relay_outcomes` carries the per-relay result map when the publish has
/// terminally settled; empty while still in flight or when the engine never
/// got that far (e.g. `NoTargets`). The iOS / Kotlin layers render this only
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
/// wire shape friendly to platforms that switch on token strings.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct RelayAckOutcome {
    pub(crate) relay_url: String,
    /// `"ok"` for an accepted relay, `"failed"` for `FailedAfterRetries`.
    pub(crate) status: String,
    /// Empty for `"ok"`; carries the engine's give-up reason for `"failed"`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) message: String,
}

/// One relay row the UI's Accounts screen edits. Mirrors the kernel's
/// per-role `RelayHealth` for the relays Pulse drives.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "codegen-schema", derive(schemars::JsonSchema))]
pub struct RelayEditRow {
    pub(crate) url: String,
    pub(crate) role: String,
    pub(crate) role_label: String,
    pub(crate) role_tint: String,
}

impl RelayEditRow {
    pub(crate) fn new(url: String, role: String) -> Self {
        let role = crate::actor::canonical_relay_role(&role).unwrap_or(role);
        let role_label = crate::actor::relay_role_label(&role);
        let role_tint = crate::actor::relay_role_tint(&role);
        Self {
            url,
            role,
            role_label,
            role_tint,
        }
    }
}

/// URLs whose relay role includes the read lane.
///
/// This is the canonical relay-role filter for any Rust host/app module that
/// needs the user's configured inbox/read relay set. Keeping it here avoids
/// platform shells re-parsing `RelayEditRow.role` tokens.
#[must_use] 
pub fn read_eligible_relay_urls(rows: &[RelayEditRow]) -> Vec<String> {
    rows.iter()
        .filter(|r| crate::actor::has_role(&r.role, "read"))
        .map(|r| r.url.clone())
        .collect()
}

/// Pre-formatted subtitle strings for the iOS Settings hub. Folds the
/// pluralization and zero-row branches into a single string so the shell
/// never duplicates the §6/AP1 "N relay(s) configured" / "No relays
/// configured" formatting.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct SettingsHubSummary {
    /// Subtitle shown under the "Relays" entry in the Settings hub.
    pub(crate) relays_subtitle: String,
}

impl SettingsHubSummary {
    /// Build the subtitle from the live relay-edit projection row count.
    /// Kept here (vs an iOS-side helper) so platforms share the same copy.
    pub(crate) fn from_relay_edit_rows(rows: &[RelayEditRow]) -> Self {
        let count = rows.len();
        let relays_subtitle = match count {
            0 => "No relays configured".to_string(),
            1 => "1 relay".to_string(),
            n => format!("{n} relays"),
        };
        Self { relays_subtitle }
    }
}

// D0: NIP-46 remote signing is an app noun, not a kernel primitive. The
// `BunkerHandshakeDto` type and its state moved out of the kernel entirely —
// they now live in the identity command runtime (`actor::commands::identity`)
// and reach the host via the `projections["bunker_handshake"]` snapshot
// projection, NOT a typed `KernelSnapshot` field. The kernel no longer holds,
// names, or projects NIP-46 handshake state.

// D0: NIP-47 NWC is an app noun, not a kernel primitive. The `WalletStatus`
// type and its state moved out of the kernel entirely — they now live in the
// wallet command runtime (`actor::commands::wallet`) and reach the host via
// the `projections["wallet"]` snapshot projection, NOT a typed `KernelSnapshot`
// field. The kernel no longer holds, names, or projects NWC wallet state.

impl super::Kernel {
    /// Replace the account projection (D4: actor is sole writer).
    pub(crate) fn set_accounts(&mut self, accounts: Vec<AccountSummary>, active: Option<String>) {
        if self.accounts != accounts || self.active_account != active {
            self.accounts = accounts;
            self.active_account = active;
            self.changed_since_emit = true;
        }
        if let Ok(mut guard) = self.active_account_handle.lock() {
            *guard = self.active_account.clone();
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
    ///
    /// This legacy uncategorized path also clears `last_error_category`: a
    /// newer toast set here must not leave a stale category from an earlier
    /// `set_error_toast_with_category` call shadowing it (iOS would branch on
    /// a category that no longer matches the visible toast). Callers that
    /// *know* the error class should use `set_error_toast_with_category`.
    pub(crate) fn set_last_error_toast(&mut self, toast: Option<String>) {
        if self.last_error_toast != toast || self.last_error_category.is_some() {
            self.last_error_toast = toast;
            self.last_error_category = None;
            self.changed_since_emit = true;
        }
    }

    /// Surface an error toast *with* a machine-readable category from the
    /// closed key set (`kernel::closed_reason::ERR_*`). iOS branches on the
    /// category without parsing the English `toast` prose. Pass the category
    /// constant, never an inline literal.
    pub(crate) fn set_error_toast_with_category(&mut self, toast: String, category: &'static str) {
        let toast = Some(toast);
        let category = Some(category.to_string());
        if self.last_error_toast != toast || self.last_error_category != category {
            self.last_error_toast = toast;
            self.last_error_category = category;
            self.changed_since_emit = true;
        }
    }

    /// Replace the editable relay projection (D4: actor is sole writer).
    /// Also syncs the shared handles so FFI-side reads
    /// and planner/publish routing see the latest rows.
    pub(crate) fn set_relay_edit_rows(&mut self, rows: Vec<RelayEditRow>) {
        let changed = self.relay_edit_rows != rows;
        if changed {
            self.relay_edit_rows = rows.clone();
            self.changed_since_emit = true;
        }
        if let Some(handle) = self.relay_edit_rows_handle.as_ref() {
            if let Ok(mut guard) = handle.lock() {
                // Typed slot — `.replace()` is the sole-writer
                // affordance defined on `RelayEditRowList`.
                guard.replace(rows.clone());
            }
        }
        let indexer_urls = rows
            .iter()
            .filter(|r| crate::actor::has_role(&r.role, "indexer"))
            .map(|r| r.url.clone())
            .collect::<Vec<_>>();
        self.lifecycle.set_indexer_relays(indexer_urls.clone());
        if let Ok(mut guard) = self.indexer_relays_handle.lock() {
            // Typed slot — `.replace()` overwrites the underlying
            // `RelayUrls(Vec<String>)` newtype.
            guard.replace(indexer_urls);
        }
        let read_urls = read_eligible_relay_urls(&rows);
        self.lifecycle.set_app_relays(read_urls.clone());
        self.lifecycle.set_active_account_read_relays(read_urls);
        // PD-033-C — the planner-extension routing lanes for kernel-driven
        // discovery oneshots. BOTH calls re-read through `bootstrap_urls_for_role`
        // so the lifecycle sees the same cold-start seeds the kernel's first
        // sockets dial (`FALLBACK_CONTENT_RELAY` / `FALLBACK_INDEXER_RELAY`
        // when no row is configured yet) — eliminating the silent-loss
        // regression Stage 1's M1 deletion would otherwise expose for both the
        // events-oneshot arm (Case D, `OneShot + Global + event_ids`) and the
        // profile-oneshot arm (Case A, `OneShot + Global + authors` with no
        // NIP-65 mailbox).
        let bootstrap_content_urls = self
            .bootstrap_urls_for_role(crate::relay::RelayRole::Content);
        self.lifecycle
            .set_bootstrap_content_relays(bootstrap_content_urls);
        let bootstrap_indexer_urls = self
            .bootstrap_urls_for_role(crate::relay::RelayRole::Indexer);
        self.lifecycle
            .set_bootstrap_indexer_relays(bootstrap_indexer_urls);
        let write_urls = rows
            .iter()
            .filter(|r| crate::actor::has_role(&r.role, "write"))
            .map(|r| r.url.clone())
            .collect::<Vec<_>>();
        if let Ok(mut guard) = self.local_write_relays_handle.lock() {
            // Typed slot — see indexer_relays_handle above.
            guard.replace(write_urls);
        }
        if changed {
            self.lifecycle.clear_probed_mailboxes();
            self.lifecycle.enqueue_trigger(
                crate::subs::CompileTrigger::UserConfiguredRelaysChanged { generation: 0 },
            );
            self.lifecycle
                .enqueue_trigger(crate::subs::CompileTrigger::IndexerSetChanged { generation: 0 });
        }
    }

    // D0: NIP-47 NWC is an app noun — `set_wallet_status` / `wallet_status_snapshot`
    // were removed with the kernel `wallet_status` field. The wallet command
    // runtime now writes wallet state to its own shared slot and the
    // `projections["wallet"]` snapshot projection surfaces it.
    //
    // D0: NIP-46 remote signing is likewise an app noun — `set_bunker_handshake`
    // / `bunker_handshake_snapshot` were removed with the kernel
    // `bunker_handshake` field. The identity command runtime writes handshake
    // state to its own shared slot and the `projections["bunker_handshake"]`
    // snapshot projection surfaces it.

    pub(crate) fn account_snapshot(&self) -> (&[AccountSummary], Option<&String>) {
        (&self.accounts, self.active_account.as_ref())
    }

    pub(crate) fn publish_queue_snapshot(&self) -> &[PublishQueueEntry] {
        &self.publish_queue
    }

    pub(crate) fn last_error_toast_snapshot(&self) -> Option<&String> {
        self.last_error_toast.as_ref()
    }

    /// Machine-readable category for `last_error_toast` (typed FFI error
    /// contract). `None` when the toast is empty or was set via the legacy
    /// uncategorized `set_last_error_toast` path.
    pub(crate) fn last_error_category_snapshot(&self) -> Option<&String> {
        self.last_error_category.as_ref()
    }

    pub(crate) fn relay_edit_rows_snapshot(&self) -> &[RelayEditRow] {
        &self.relay_edit_rows
    }

}

/// Abbreviated npub: `<first10>…<last8>` for values longer than 20 chars;
/// the value verbatim otherwise. Mirrors the `profile_npub_short` policy
/// in `kernel/update.rs` byte-for-byte. Lives here (not as a re-import)
/// so `identity_state.rs` does not depend on a private helper inside the
/// sibling `update.rs` — keeps the kernel modules independently testable
/// and respects the V-22 precedent of accepting a single line of micro-
/// duplication over a cross-module coupling.
pub(crate) fn account_npub_short(npub: &str) -> String {
    let count = npub.chars().count();
    if count <= 20 {
        return npub.to_string();
    }
    let prefix: String = npub.chars().take(10).collect();
    let suffix: String = npub
        .chars()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{prefix}…{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(url: &str, role: &str) -> RelayEditRow {
        RelayEditRow::new(url.to_string(), role.to_string())
    }

    #[test]
    fn account_npub_short_returns_value_verbatim_under_20_chars() {
        assert_eq!(account_npub_short(""), "");
        assert_eq!(account_npub_short("abc"), "abc");
        let twenty = "a".repeat(20);
        assert_eq!(account_npub_short(&twenty), twenty);
    }

    #[test]
    fn account_npub_short_truncates_long_value_first10_last8_with_ellipsis() {
        let npub = "npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft";
        let out = account_npub_short(npub);
        assert_eq!(out, "npub1l2vyh…fqutajft");
        assert!(out.contains('…'));
    }

    #[test]
    fn read_eligible_relay_urls_accepts_read_and_both() {
        let rows = vec![
            row("wss://read.example", "read"),
            row("wss://both.example", "both"),
            row("wss://write.example", "write"),
            row("wss://index.example", "indexer"),
        ];
        assert_eq!(
            read_eligible_relay_urls(&rows),
            vec!["wss://read.example", "wss://both.example"]
        );
    }

    #[test]
    fn read_eligible_relay_urls_uses_canonical_role_tokens() {
        let rows = vec![
            row("wss://composite.example", "write + indexer + read"),
            row("wss://upper.example", "BOTH,INDEXER"),
            row("wss://not-read.example", "writer"),
        ];
        assert_eq!(
            read_eligible_relay_urls(&rows),
            vec!["wss://composite.example", "wss://upper.example"]
        );
    }
}
