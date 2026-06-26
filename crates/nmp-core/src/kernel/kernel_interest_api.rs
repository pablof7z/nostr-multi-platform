//! Start/visibility/emit flags + generic interest open/close.
//!
//! Extracted from `kernel/mod.rs` (`impl Kernel`) to honour the 500-LOC ceiling.

use super::*;

impl Kernel {
    pub(crate) fn start(&mut self) {
        if self.timing.started_at.is_none() {
            self.timing.started_at = Some(Instant::now()); // doctrine-allow: D9 — status diagnostic elapsed-time marker; wall anchor uses now_ms below
            self.timing.started_unix_ms = Some(self.now_ms()); // D9 wall anchor
        }
        self.changed_since_emit = true;
        self.log("starting role-aware nmp demo slice");
    }

    pub(crate) fn set_visible_limit(&mut self, limit: usize) {
        if self.visible_limit != limit {
            self.visible_limit = limit;
            self.changed_since_emit = true;
        }
    }

    pub(crate) fn visible_limit(&self) -> usize {
        self.visible_limit
    }

    pub(crate) fn changed_since_emit(&self) -> bool {
        self.changed_since_emit
    }

    /// Force the next due tick to emit a snapshot even if no kernel field changed.
    pub fn mark_changed_since_emit(&mut self) {
        self.changed_since_emit = true;
    }

    /// Mutable access to the subscription lifecycle (registry + trigger inbox).
    pub(crate) fn lifecycle_mut(&mut self) -> &mut SubscriptionLifecycle {
        &mut self.lifecycle
    }

    /// Ensure-absent interest registration (#2045 PR-A).
    ///
    /// Registers the `(identity, interest)` pair if absent (EnsureAbsent policy);
    /// store-serve + recompile trigger fire only when the interest is newly installed.
    /// Used by the `EnsureInterest` `ActorCommand` arm (native dispatch AND the
    /// headless `apply_actor_command` interpreter) to preserve the exact reason
    /// tag `"ensure-interest"` that diagnostics downstream key on.
    pub(crate) fn ensure_interest(
        &mut self,
        identity: crate::subs::SubIdentity,
        interest: crate::planner::LogicalInterest,
    ) {
        self.register_interest(
            &[crate::kernel::cache_serve::InterestRegistration {
                identity,
                interest,
                policy: crate::kernel::cache_serve::InterestWrite::EnsureAbsent,
            }],
            "ensure-interest",
        );
    }

    /// Drop one owner from the registry; enqueues `InvalidateCompile` if it was
    /// removed (#2045 PR-A).
    ///
    /// Returns `true` when the owner was present and removed. Reason tag:
    /// `"drop-interest-owner"` (preserved verbatim from the original
    /// `cmd_interests::drop_interest_owner` dispatch arm).
    pub(crate) fn drop_interest_owner(&mut self, identity: crate::subs::SubIdentity) -> bool {
        let removed = self.lifecycle.registry_mut().drop_owner(&identity);
        if removed {
            self.lifecycle
                .enqueue_trigger(crate::subs::CompileTrigger::InvalidateCompile {
                    reason: crate::subs::InvalidateReason::External(
                        "drop-interest-owner".to_string(),
                    ),
                });
        }
        removed
    }

    /// M2 (ADR-0042) — attach one owner to a generic feed interest; enqueues a recompile trigger.
    pub(crate) fn open_interest_sub(
        &mut self,
        identity: crate::subs::SubIdentity,
        interest: crate::planner::LogicalInterest,
    ) -> bool {
        // Unified front-door (EnsureAbsent = register-if-absent). Store-serve +
        // recompile trigger fire only when the interest is newly installed.
        let outcomes = self.register_interest(
            &[crate::kernel::cache_serve::InterestRegistration {
                identity,
                interest,
                policy: crate::kernel::cache_serve::InterestWrite::EnsureAbsent,
            }],
            "open-interest",
        );
        outcomes[0].newly_installed
    }

    /// M2 (ADR-0042) — detach one owner from a generic feed interest; enqueues a recompile trigger.
    pub(crate) fn close_interest_sub(&mut self, identity: &crate::subs::SubIdentity) -> bool {
        let removed = self.lifecycle.registry_mut().drop_owner(identity);
        if removed {
            self.lifecycle
                .enqueue_trigger(crate::subs::CompileTrigger::InvalidateCompile {
                    reason: crate::subs::InvalidateReason::External("close-interest".to_string()),
                });
        }
        removed
    }
}
