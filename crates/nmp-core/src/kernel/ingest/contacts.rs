//! Contact-list transition hooks.
//!
//! The protocol-owned contact-list reader is the source of contact facts.
//! `nmp-core` observes active-account contact transitions and wakes the generic
//! subscription compiler. Dynamic feed sessions own any reduced-source
//! re-expansion through their registered observers and dependent-interest sets.

use super::super::{short_hex, Kernel};
use crate::subs::{AccountId, CompileTrigger};

impl Kernel {
    /// Active-account contact-list transition.
    ///
    /// When `project_accepted_event` sees the active account's contact set
    /// transition, this hook logs it and enqueues one compile trigger. It does
    /// not register a follow-feed interest; those are owned by reduced-source
    /// sessions above core.
    pub(in crate::kernel) fn on_active_contacts_changed(
        &mut self,
        author: &str,
        follows: Vec<String>,
        _created_at: u64,
    ) {
        self.log(format!(
            "contacts {} -> {} followees",
            short_hex(author),
            follows.len()
        ));

        self.lifecycle
            .enqueue_trigger(CompileTrigger::FollowListChanged {
                account_id: AccountId(author.to_string()),
                new_follows: follows,
            });
    }

    /// Identity switch/logout hook for feed-source cache serve state.
    ///
    /// Account-scoped reduced-source sessions re-resolve through their own
    /// identity observers, but the kernel's cache-serve completion set is also
    /// account-sensitive. Clear it on identity change so newly recompiled
    /// account-scoped interests get a fresh store serve and stale queued serves
    /// from the previous account are dropped.
    pub(crate) fn reconcile_feed_sources_after_identity_change(&mut self) {
        self.clear_served_interest_shapes();
        self.timeline_authors.clear();
        self.lifecycle
            .enqueue_trigger(CompileTrigger::InvalidateCompile {
                reason: crate::subs::InvalidateReason::External("identity-changed".to_string()),
            });
    }
}
