//! Shared post-store projection fan-out for live ingest and cache-serve replay.

use super::super::Kernel;
use super::helpers;

impl Kernel {
    /// ADR-0070 — the SINGLE shared post-store projection fan-out, called by
    /// BOTH the live ingest chokepoint ([`Self::ingest_accepted_event`]) AND the
    /// cache-serve replay path ([`Self::feed_served_event`]).
    ///
    /// It owns the three post-store concerns, kind-agnostically:
    /// NIP-parser dispatch, capability-cache transition sweep, and D9-clamped
    /// app-observer notification. Callers MUST gate on the canonical accepted
    /// outcome (`Inserted | Replaced | Ephemeral`).
    pub(in crate::kernel) fn project_accepted_event(
        &mut self,
        verified: &crate::store::VerifiedEvent,
    ) {
        self.project_accepted_event_from(verified, None, None);
    }

    /// Source-aware variant for live relay ingest. Cache replay and local
    /// publish use [`Self::project_accepted_event`] so parser-owned read models
    /// do not confuse replay/local rows with relay provenance.
    pub(in crate::kernel) fn project_accepted_event_from(
        &mut self,
        verified: &crate::store::VerifiedEvent,
        source_relay_url: Option<&str>,
        active_contacts_before: Option<Option<Vec<String>>>,
    ) {
        let raw = verified.raw();
        let author = raw.pubkey.clone();
        let event_id = raw.id.clone();
        let created_at_for_trigger = raw.created_at;

        // (2a) Snapshot the capability caches BEFORE the parser dispatch writes
        // them, kind-agnostically.
        let mailbox_before = self.mailbox_cache().snapshot(&author);
        let dm_before = self.recipient_dm_relays(&author);
        let profile_before = self.profile_lookup().profile(&author);
        // GAP-3: snapshot the active account's blocked-relay set BEFORE dispatch
        // so we can detect a kind:10006 write and enqueue a recompile.
        let blocked_before_urls: std::collections::BTreeSet<String> =
            self.snapshot_blocked_relays().iter().cloned().collect();

        // (1) NIP-parser dispatch. D6 — a poisoned dispatcher lock degrades to
        // "no parser fired" (graceful; persistence already succeeded).
        if let Ok(d) = self.ingest_dispatcher_slot().read() {
            d.dispatch_at_source(verified, self.now_secs(), source_relay_url);
        }

        // (2b) Transition sweep AFTER dispatch.
        let profile_after = self.profile_lookup().profile(&author);
        if profile_before != profile_after {
            self.cached_estimated_store_bytes.set(None);
            self.projection_rev_tracker.source_versions.bump_profiles();
            // ADR-0070 Lane B (D6a) — per-key rev (ingest site 3 of 3): a kind:0
            // rewrote this author's row, so bump ITS rev (and only its) when the
            // author is a live `refs.profile` ref. Gating on a live claim keeps the
            // per-key map bounded to resolved refs (unclaimed authors never enter
            // `refs.profile`, so their row rev is never consulted). Claimed
            // profiles are pinned against RAM eviction (`ram_eviction.rs`), so the
            // eviction `bump_profiles` site needs no per-key counterpart.
            if self.profile_claims.contains_key(&author) {
                self.projection_rev_tracker
                    .source_versions
                    .bump_profile_row(&author);
            }
        }
        let mailbox_after = self.mailbox_cache().snapshot(&author);
        if mailbox_before != mailbox_after {
            self.on_mailbox_changed(&author, &event_id, created_at_for_trigger);
        }
        let dm_after = self.recipient_dm_relays(&author);
        if dm_before != dm_after {
            self.on_dm_relays_changed(&author, created_at_for_trigger);
        }
        if let Some(active_contacts_before) = active_contacts_before {
            let active_contacts_after = self.contact_list_reader().follows(&author);
            if active_contacts_before != active_contacts_after {
                if let Some(follows) = active_contacts_after {
                    self.on_active_contacts_changed(&author, follows, created_at_for_trigger);
                }
            }
        }
        // GAP-3: detect blocked-relay-set change. Kind:10006 is the wire shape,
        // but the transition detector is kind-agnostic (mirrors the contacts
        // pattern above). When the active account's blocked set changes, enqueue
        // a recompile so SPLIT B removes the newly-blocked relay on the next drain.
        // Only check for the active account — the blocked set is account-scoped.
        if self.active_account.as_deref() == Some(author.as_str()) {
            let blocked_after_urls: std::collections::BTreeSet<String> =
                self.snapshot_blocked_relays().iter().cloned().collect();
            if blocked_before_urls != blocked_after_urls {
                self.on_blocked_relays_changed();
            }
        }

        // (3) D9-clamped app-observer notify.
        self.notify_observers_for_verified_event_with_provenance(verified, None);
    }

    /// Deliver a verified wire event to app observers with optional arrival
    /// provenance, without feeding parser-owned caches.
    ///
    /// Stored events normally recover provenance from the event store in
    /// `notify_event_observers`; rejected expired-on-arrival events never enter
    /// the store, so relay-pinned observed projections need the arrival source
    /// carried on the event.
    pub(in crate::kernel) fn notify_observers_for_verified_event_with_provenance(
        &self,
        verified: &crate::store::VerifiedEvent,
        provenance: Option<&str>,
    ) {
        let now_secs = self.now_secs();
        let mut kernel_event = helpers::kernel_event_from_verified(verified);
        kernel_event.created_at = kernel_event.created_at.min(now_secs);
        if let Some(provenance) = provenance {
            kernel_event.relay_provenance.push(provenance.to_string());
        }
        self.notify_event_observers(&kernel_event);
    }

    pub(in crate::kernel) fn active_contacts_snapshot_for_author(
        &self,
        author: &str,
    ) -> Option<Option<Vec<String>>> {
        (self.active_account.as_deref() == Some(author))
            .then(|| self.contact_list_reader().follows(author))
    }

    /// Wall-clock arrival timestamp (unix millis) for a store insert.
    ///
    /// Clock seam (kernel/clock.rs): `received_at_ms` is reducer output —
    /// it is written into the `EventStore` — so it MUST read the injected
    /// `Clock` rather than `SystemTime::now()` directly, otherwise
    /// deterministic replay diverges (D9: the kernel owns time).
    pub(in crate::kernel) fn ingest_received_at_ms(&self) -> u64 {
        self.clock
            .now()
            .duration_since(super::super::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Substrate-honest mailbox-change observer (replaces the deleted
    /// `kernel/ingest/relay_list.rs` impl, 2026-05-25).
    pub(in crate::kernel) fn on_mailbox_changed(
        &mut self,
        author: &str,
        event_id: &str,
        created_at: u64,
    ) {
        let _ = self.route_subscription_relays(
            crate::stable_hash::stable_hash64(("mailbox-changed", event_id, created_at)),
            &[author],
            &[], // V-68/D0: no substrate social default; trace lane is kind-independent.
            super::super::mailboxes::BootstrapSeed::Discovery,
        );
        self.lifecycle
            .enqueue_trigger(crate::subs::CompileTrigger::Nip65Arrived {
                pubkey: author.to_string(),
                created_at,
            });
    }

    /// F-02 — substrate-honest DM-relay-list-change observer.
    pub(in crate::kernel) fn on_dm_relays_changed(&mut self, author: &str, created_at: u64) {
        self.lifecycle
            .enqueue_trigger(crate::subs::CompileTrigger::DmRelayListChanged {
                pubkey: author.to_string(),
                created_at,
            });
    }

    /// GAP-3 — blocked-relay-set-change observer.
    ///
    /// Called when the active account's kind:10006 blocked-relay list is
    /// updated by an ingest parser (typically `Kind10006Parser` in `nmp-nip51`).
    /// Enqueues an `InvalidateCompile` trigger so System-A's SPLIT B re-runs on
    /// the next drain and drops the newly-blocked relay's REQ from the wire plan.
    fn on_blocked_relays_changed(&mut self) {
        self.lifecycle
            .enqueue_trigger(crate::subs::CompileTrigger::InvalidateCompile {
                reason: crate::subs::InvalidateReason::External(
                    "blocked-relays-changed".to_string(),
                ),
            });
    }
}
