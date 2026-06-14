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

use crate::publish::PublishTarget;
use crate::substrate::SignedEvent;
use serde::Serialize;

/// Shared slot for the currently active account pubkey.
///
/// Follows the same typed-slot pattern as [`IndexerRelaysSlot`] and
/// [`LocalWriteRelaysSlot`] in `relay_projection`: a named type alias prevents
/// accidental bare `Arc<Mutex<Option<String>>>` proliferation and lets D14's
/// lint catch shape regressions at the declaration site rather than silently at
/// every call site.
///
/// `pub` (widened from `pub(crate)` 2026-05-25, spec §271): re-exported
/// through `crate::slots` so `nmp-router::Nip65OutboxResolver` can name the
/// slot type. The slot is opaque (no public mutator) so widening visibility
/// does not invert the D4 sole-writer invariant — the actor side stays the
/// only writer; external readers `lock()` + read `.clone()`.
pub type ActiveAccountSlot = Arc<Mutex<Option<String>>>;

/// Construct a fresh, empty [`ActiveAccountSlot`].
///
/// `pub` (widened from `pub(crate)` 2026-05-25, spec §271): re-exported
/// through `crate::slots` for `nmp-router::Nip65OutboxResolver` composition.
pub fn new_active_account_slot() -> ActiveAccountSlot {
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
    /// Bech32 `npub1…` encoding of `id`. Pubkey-deterministic; presentation
    /// layer chooses how to abbreviate.
    pub(crate) npub: String,
    /// Display name from kind:0 (`display_name` / `displayName` / `name`,
    /// first non-empty wins). `None` when no kind:0 has been received yet
    /// — presentation layer chooses how to render the missing case
    /// (typically by formatting the raw pubkey itself) — see aim.md §2.
    pub(crate) display_name: Option<String>,
    pub(crate) signer_kind: String,
    /// `"active"` for the active account, `"idle"` otherwise.
    pub(crate) status: String,
    /// Pre-classified, human-readable signer label (e.g. `"nsec"`,
    /// `"NIP-46"`). Free-form signer classification; the host renders this
    /// verbatim instead of switching on `signer_kind`.
    pub(crate) signer_label: String,
    /// `true` when the signer's key material lives outside the kernel
    /// (NIP-46 bunker today, NIP-07 / hardware later). Lets native scope
    /// remote-signer-only sections without string-matching `signer_kind`.
    pub(crate) signer_is_remote: bool,
    /// Pre-derived `status == "active"`. Native binds this directly.
    pub(crate) is_active: bool,
    /// Profile picture URL from kind:0 metadata. `None` when no kind:0
    /// has been received yet or the metadata carries no `picture` field;
    /// enriched by `Kernel::accounts_enriched()` in the snapshot builder.
    /// Presentation layer chooses a placeholder/identicon strategy for
    /// the missing case (aim.md §2).
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
    /// Pre-formatted English label for `kind` (e.g. `"Note"`, `"Reaction"`).
    /// Mirrors the in-flight `PublishOutboxItem.title` so apps render a
    /// consistent kind label across the active outbox and the settled
    /// history pane. Owned by the kernel — apps render verbatim and never
    /// reimplement a kind→label mapping (RMP bible commandment #4).
    pub(crate) title: String,
    pub(crate) target_relays: usize,
    pub(crate) status: String,
    /// Rust-owned action decision: a shell renders/enables Retry directly
    /// instead of reconstructing retry policy from status strings.
    pub(crate) can_retry: bool,
    /// Per-relay terminal outcomes, in insertion order. Empty while
    /// `status == "accepted_locally"` (no terminal verdict yet).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) relay_outcomes: Vec<RelayAckOutcome>,
    /// Internal retry payload for settled failures. Skipped from snapshots:
    /// hosts address retries by handle, never by re-submitting event JSON.
    #[serde(skip)]
    pub(crate) signed_event: Option<SignedEvent>,
    /// Internal retry target paired with `signed_event`.
    #[serde(skip)]
    pub(crate) target: Option<PublishTarget>,
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
    /// Per-relay selection rationale captured at publish time (e.g.
    /// `"NIP-65 write relay"`, `"Inbox relay for <hex pubkey>"`). The raw hex
    /// pubkey is emitted verbatim by the kernel projection — D6 forbids
    /// `display::*` abbreviation helpers in the projection layer; the shell
    /// applies any `short_npub` / bech32 rendering it wants. Mirrors the
    /// in-flight `publish_outbox` field so the settled `publish_queue`
    /// projection can render the same "why was this relay targeted?" string
    /// after the publish has completed. Empty when the engine had no
    /// rationale for the relay (e.g. older serialised rows resumed from
    /// store).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(crate) relay_reason: String,
}

/// One relay row the UI's Accounts screen edits. Mirrors the kernel's
/// per-role `RelayHealth` for the relays Pulse drives.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "codegen-schema", derive(schemars::JsonSchema))]
pub struct AppRelay {
    pub(crate) url: String,
    pub(crate) role: String,
}

impl AppRelay {
    pub(crate) fn new(url: String, role: String) -> Self {
        let role = crate::actor::canonical_relay_role(&role).unwrap_or(role);
        Self { url, role }
    }

    /// Borrow the relay URL string.
    ///
    /// Read-only accessor so external readers (the `nmp-ffi` shell, per-app
    /// crates) can iterate the relay-edit projection without naming the
    /// crate-private `url` field directly. The actor is the sole writer
    /// (D4); no setter exists.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Borrow the relay role string (canonicalised — `read`, `write`,
    /// `both`, `indexer`, or a composite like `both,indexer`).
    ///
    /// Read-only accessor; same reasoning as [`Self::url`].
    #[must_use]
    pub fn role(&self) -> &str {
        &self.role
    }
}

/// URLs whose relay role includes the read lane.
///
/// This is the canonical relay-role filter for any Rust host/app module that
/// needs the user's configured inbox/read relay set. Keeping it here avoids
/// platform shells re-parsing `AppRelay.role` tokens.
#[must_use]
pub fn read_eligible_relay_urls(rows: &[AppRelay]) -> Vec<String> {
    rows.iter()
        .filter(|r| crate::actor::has_role(&r.role, "read"))
        .map(|r| r.url.clone())
        .collect()
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
            let active_changed = self.active_account != active;
            self.accounts = accounts;
            self.active_account = active;
            self.changed_since_emit = true;
            // ADR-0055 Rung 1: bump source version counters.
            self.projection_rev_tracker.source_versions.bump_accounts();
            if active_changed {
                self.projection_rev_tracker.source_versions.bump_active_account();
                // ADR-0055 Rung 1 (F6): an account switch invalidates every
                // account-scoped projection cache on the host — bump the epoch so
                // Rung 3's host re-baselines all projections (treats the next emit
                // as a full snapshot, not a delta).
                self.projection_rev_tracker.bump_epoch();
            }
        }
        if let Ok(mut guard) = self.active_account_handle.lock() {
            *guard = self.active_account.clone();
        }
    }

    /// Lightweight active-account setter for the wasm path.
    ///
    /// The native actor uses [`Self::set_accounts`] (which requires a full
    /// `Vec<AccountSummary>` driven by `actor::commands::identity`). The wasm
    /// runtime has no account-management actor; when the NIP-07 signer installs
    /// itself it already knows the viewer pubkey from
    /// `window.nostr.getPublicKey()` — this method feeds that pubkey into the
    /// kernel so contact-feed resolution and bootstrap interests know whose
    /// follows to load.
    ///
    /// Sets `active_account` and flushes the `active_account_handle` mutex slot
    /// (the same two writes `set_accounts` does for the active field). Does NOT
    /// touch `self.accounts` (the typed account-list projection) — on the wasm
    /// path that projection stays empty and the host does not render it.
    pub(crate) fn set_active_account(&mut self, pubkey: String) {
        let changed = self.active_account.as_deref() != Some(pubkey.as_str());
        self.active_account = Some(pubkey);
        self.changed_since_emit = true;
        if changed {
            // ADR-0055 Rung 1: bump active_account_ver (wasm path — no full accounts vec).
            self.projection_rev_tracker.source_versions.bump_active_account();
            // ADR-0055 Rung 1 (F6): account switch → epoch bump (host re-baseline).
            self.projection_rev_tracker.bump_epoch();
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
        // ADR-0055 Rung 1: bump publish_ver.
        self.projection_rev_tracker.source_versions.bump_publish();
    }

    pub(crate) fn remove_publish_entry(&mut self, event_id: &str) -> bool {
        let Some(idx) = self
            .publish_queue
            .iter()
            .rposition(|entry| entry.event_id == event_id)
        else {
            return false;
        };
        self.publish_queue.remove(idx);
        self.changed_since_emit = true;
        // ADR-0055 Rung 1: bump publish_ver.
        self.projection_rev_tracker.source_versions.bump_publish();
        true
    }

    pub(crate) fn retry_payload_for_publish(
        &self,
        event_id: &str,
    ) -> Option<(SignedEvent, PublishTarget)> {
        self.publish_queue
            .iter()
            .rev()
            .find(|entry| entry.event_id == event_id && entry.can_retry)
            .and_then(|entry| Some((entry.signed_event.clone()?, entry.target.clone()?)))
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
        entry.can_retry = publish_entry_can_retry(status, &outcomes, entry.signed_event.is_some());
        entry.relay_outcomes = outcomes;
        self.changed_since_emit = true;
        // ADR-0055 Rung 1: bump publish_ver on terminal state transition.
        self.projection_rev_tracker.source_versions.bump_publish();
    }

    /// Surface a coarse error string to the UI (D6: errors are state, never
    /// exceptions across FFI). `None` clears the toast.
    ///
    /// This legacy uncategorized path also clears `last_error_category`: a
    /// newer toast set here must not leave a stale category from an earlier
    /// `set_error_toast_with_category` call shadowing it (iOS would branch on
    /// a category that no longer matches the visible toast). Callers that
    /// *know* the error class should use `set_error_toast_with_category`.
    pub fn set_last_error_toast(&mut self, toast: Option<String>) {
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
    pub(crate) fn set_configured_relays(&mut self, rows: Vec<AppRelay>) {
        let changed = self.configured_relays != rows;
        if changed {
            self.configured_relays = rows.clone();
            self.changed_since_emit = true;
            // ADR-0055 Rung 1: bump configured_relays_ver.
            // (diagnostics_inputs_ver is NOT co-bumped here — F5 derives it from the
            // relay_diagnostics payload fingerprint each emit, not per mutation site.)
            self.projection_rev_tracker.source_versions.bump_configured_relays();
        }
        if let Some(handle) = self.configured_relays_handle.as_ref() {
            if let Ok(mut guard) = handle.lock() {
                // Typed slot — `.replace()` is the sole-writer
                // affordance defined on `AppRelayList`.
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
        let bootstrap_content_urls = self.bootstrap_urls_for_role(crate::relay::RelayRole::Content);
        self.lifecycle
            .set_bootstrap_content_relays(bootstrap_content_urls);
        let bootstrap_indexer_urls = self.bootstrap_urls_for_role(crate::relay::RelayRole::Indexer);
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

    pub(crate) fn configured_relays_snapshot(&self) -> &[AppRelay] {
        &self.configured_relays
    }
}

pub(in crate::kernel) fn publish_entry_can_retry(
    status: &str,
    outcomes: &[RelayAckOutcome],
    has_retry_payload: bool,
) -> bool {
    if !has_retry_payload {
        return false;
    }
    status == "failed"
        || status == "pending_relays_unknown"
        || outcomes.iter().any(|relay| relay.status == "failed")
}

#[cfg(test)]
#[path = "identity_state/tests.rs"]
mod tests;
