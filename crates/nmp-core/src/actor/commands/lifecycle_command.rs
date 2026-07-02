//! `LifecycleCommand` — actor + app lifecycle verbs (ADR-0071).
//!
//! Grouped under `ActorCommand::Lifecycle(LifecycleCommand)`. Dispatch home:
//! `actor/dispatch/cmd_lifecycle.rs`.

use crate::kernel::LifecyclePhase;

/// Actor + app lifecycle commands: start/stop/reset/shutdown + app phase
/// transitions + the mark-dirty hook for host-registered projections.
#[derive(Debug)]
pub enum LifecycleCommand {
    Start {
        visible_limit: usize,
        emit_hz: u32,
        /// App-declared initial relay configuration. Seeded into
        /// `configured_relays` before the session restore runs. Empty for
        /// C-ABI callers that seed via pre-start `add_relay` calls instead.
        initial_relays: Vec<(String, String)>,
    },
    Configure {
        visible_limit: usize,
        emit_hz: u32,
    },
    /// T118 / G3 — app lifecycle phase transition reported by the host shell
    /// (or any conforming consumer). The actor folds the phase into the
    /// kernel's [`crate::kernel::LifecyclePhase`] state and, on a meaningful
    /// transition (`Background → Foreground`, `Foreground → Background`, or
    /// first phase after boot), fires the registered lifecycle observer.
    /// Idempotent: rapid scene oscillation debounces to a single observer call
    /// per transition. D0: nmp-core names no shell vocabulary.
    LifecycleEvent(LifecyclePhase),
    /// Mark the kernel dirty so host-registered snapshot projections re-emit.
    ///
    /// Used when reusable NMP extension state changes outside a typed kernel
    /// field (for example a registered feed viewport expanding older rows).
    MarkChangedSinceEmit,
    Stop,
    Reset,
    Shutdown,
}
