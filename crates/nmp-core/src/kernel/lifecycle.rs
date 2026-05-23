//! App lifecycle phase tracking (T118 / G3 from relay-lifecycle review).
//!
//! The kernel models three phases reflecting iOS `scenePhase` semantics:
//! [`LifecyclePhase::Foreground`] (`.active`), [`LifecyclePhase::Background`]
//! (`.background`), and [`LifecyclePhase::Inactive`] (transient, also the
//! initial pre-FFI-report value).
//!
//! ## Why kernel-side, not native?
//!
//! D7 (capability vs policy): the OS reports the *fact* of scenePhase changes
//! via the FFI; the kernel decides what *meaning* a phase has — when to
//! reconcile NIP-77 watermarks, when to back off retries, when to keep
//! sockets open. Native iOS code merely tells the truth (`.active` →
//! `nmp_app_lifecycle_foreground`); every decision derived from that fact
//! lives here.
//!
//! ## D0 boundary
//!
//! A shell-side sync-trigger engine carries the `Foreground` notion, but
//! `nmp-core` does NOT depend on any such crate (would be a dep cycle — any
//! such crate already consumes `nmp-core::store`). The kernel therefore
//! exposes a callback observer (`ffi::lifecycle::nmp_app_set_lifecycle_callback`)
//! that a consumer (the Pulse/Stress app or an integration test) registers
//! to receive transitions and fan them out to its own trigger engine.
//! Mirrors the established `capability_callback` pattern
//! (`ffi/capability.rs`).
//!
//! ## Idempotence (D6 contract)
//!
//! The transition observer ONLY fires on a meaningful state change:
//! Background → Foreground (the trigger-bearing transition) and
//! Foreground → Background. Repeated `Foreground` events (rapid scenePhase
//! oscillation from a back-foreground swipe) collapse to a single observer
//! callback. Repeated `Background` events likewise.
//!
//! From the initial [`LifecyclePhase::Inactive`] (kernel never told a phase)
//! the FIRST `Foreground` *does* fire an observer call: it covers the boot
//! case where the app launches into the foreground and the reconciler needs
//! a kickoff.

/// Phase reported by the native shell via the FFI. The kernel is the sole
/// authority on what each phase *means* (D7); the shell only reports the OS
/// event.
///
/// `pub` (not `pub(crate)`) because it's carried by
/// [`crate::actor::ActorCommand::LifecycleEvent`], which is `pub`. The
/// `kernel` module itself is crate-private, so external Rust callers reach
/// this only via the `lib.rs` test-support re-export path; production
/// callers cross the FFI seam and never name the Rust type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecyclePhase {
    /// Initial state — the shell has never reported a phase. Also covers
    /// transient `.inactive` from iOS (transitioning between active and
    /// background). No side-effects derive from this phase; transitions
    /// *to* `Inactive` from any other phase are silently no-op'd in the
    /// transition logic (the next concrete phase is what matters).
    Inactive,
    /// App is in the background (iOS `.background`). The kernel may defer
    /// non-urgent retries here (today this is a no-op — the policy lives in
    /// the actor's existing tick loop). Future work: close idle sockets
    /// after a grace period without violating the T126 one-socket-per-URL
    /// invariant.
    Background,
    /// App is in the foreground (iOS `.active`). The kernel fans out a
    /// foreground trigger via the registered observer so NIP-77's
    /// `TriggerEvent::Foreground` reconciles every open `(filter, relay)`
    /// against its persisted watermark.
    Foreground,
}

/// Outcome of a [`Kernel::set_lifecycle_phase`] call. `None` means the
/// request did not represent a meaningful transition (e.g. `Foreground →
/// Foreground` debounce); `Some(LifecycleTransition::…)` carries a verdict
/// the actor uses to decide whether to invoke the registered observer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LifecycleTransition {
    /// New phase is [`LifecyclePhase::Foreground`] and the previous phase was
    /// NOT `Foreground` (i.e. `Inactive` or `Background`). The observer
    /// should fire with `Foreground` so consumers can dispatch
    /// `TriggerEvent::Foreground` through their `TriggerEngine`.
    EnteredForeground,
    /// New phase is [`LifecyclePhase::Background`] and the previous phase was
    /// NOT `Background` (i.e. `Inactive` or `Foreground`). The observer
    /// fires with `Background`; today no native consumer reacts (NIP-77 has
    /// no `Background` trigger variant), but the hook is symmetric so
    /// future work can wire socket close-after-grace-period policy.
    EnteredBackground,
}

impl LifecyclePhase {
    /// Compute the transition produced by moving from `prev` to `self`.
    /// Returns `None` for non-meaningful transitions (debounce, or to/from
    /// `Inactive`).
    ///
    /// `Inactive` is purely the "haven't heard" sentinel: transitioning *to*
    /// `Inactive` is a silent no-op (iOS hits `.inactive` during every
    /// app-switch animation; trying to do work there would double-fire).
    /// Transitioning *from* `Inactive` to a concrete phase is treated as
    /// the first observed phase event — `Inactive → Foreground` IS an
    /// `EnteredForeground`, covering the boot-into-foreground case.
    pub(crate) fn transition_from(self, prev: Self) -> Option<LifecycleTransition> {
        match (prev, self) {
            // Idempotence: same → same is always a no-op.
            (a, b) if a == b => None,
            // Transitioning TO Inactive is a no-op (iOS interstitial state).
            (_, Self::Inactive) => None,
            // Any → Foreground (when prev wasn't Foreground): trigger fan-out.
            (_, Self::Foreground) => Some(LifecycleTransition::EnteredForeground),
            // Any → Background (when prev wasn't Background): symmetric hook.
            (_, Self::Background) => Some(LifecycleTransition::EnteredBackground),
        }
    }
}

impl crate::kernel::Kernel {
    /// Record a new lifecycle phase and return the transition verdict (or
    /// `None` if the call is a no-op). Idempotent on repeated phases (the
    /// rapid scene-phase oscillation case). Crate-private — the actor is
    /// the sole caller (D4 single-writer).
    pub(crate) fn set_lifecycle_phase(
        &mut self,
        new_phase: LifecyclePhase,
    ) -> Option<LifecycleTransition> {
        let transition = new_phase.transition_from(self.lifecycle_phase)?;
        self.lifecycle_phase = new_phase;
        // Log inside the early-return guard so we never need to coerce a
        // `None` transition into a string (D6: nothing surfaces as an
        // exception or panic — including an `unreachable!`).
        self.log(match transition {
            LifecycleTransition::EnteredForeground => "lifecycle: entered foreground",
            LifecycleTransition::EnteredBackground => "lifecycle: entered background",
        });
        Some(transition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_to_foreground_is_meaningful() {
        assert_eq!(
            LifecyclePhase::Foreground.transition_from(LifecyclePhase::Inactive),
            Some(LifecycleTransition::EnteredForeground),
        );
    }

    #[test]
    fn boot_to_background_is_meaningful() {
        // Edge case: shell tells us we launched straight into background.
        // No native consumer reacts today, but the transition IS valid.
        assert_eq!(
            LifecyclePhase::Background.transition_from(LifecyclePhase::Inactive),
            Some(LifecycleTransition::EnteredBackground),
        );
    }

    #[test]
    fn rapid_double_foreground_debounces() {
        assert_eq!(
            LifecyclePhase::Foreground.transition_from(LifecyclePhase::Foreground),
            None,
        );
    }

    #[test]
    fn rapid_double_background_debounces() {
        assert_eq!(
            LifecyclePhase::Background.transition_from(LifecyclePhase::Background),
            None,
        );
    }

    #[test]
    fn background_then_foreground_fires_once() {
        // The trigger-bearing transition the whole task exists to cover.
        assert_eq!(
            LifecyclePhase::Foreground.transition_from(LifecyclePhase::Background),
            Some(LifecycleTransition::EnteredForeground),
        );
    }

    #[test]
    fn inactive_target_is_always_a_noop() {
        assert_eq!(
            LifecyclePhase::Inactive.transition_from(LifecyclePhase::Foreground),
            None,
        );
        assert_eq!(
            LifecyclePhase::Inactive.transition_from(LifecyclePhase::Background),
            None,
        );
    }
}
