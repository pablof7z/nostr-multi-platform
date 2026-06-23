//! Failure taxonomy for [`super::ActionRegistry`] errors.
//!
//! Extracted from `action_registry.rs` to keep that orchestrator file under the
//! 500-LOC hand-authored ceiling (AGENTS.md / V-12).
//!
//! Contains two failure types:
//! * [`RegistrationError`] — structured duplicate-namespace error returned by
//!   [`super::ActionRegistry::register`] (#1724).
//! * [`ActionExecuteFailure`] — failure taxonomy for
//!   [`super::ActionRegistry::execute`] (#1676 BUG-B).

/// Structured error returned when an app-over-app namespace collision is
/// detected at registration time (#1724).
///
/// Replaces the `debug_assert!` in [`super::ActionRegistry::register`] so the
/// collision is a structured, inspectable error in BOTH dev AND release builds,
/// not a dev-only assertion that silently last-writes in production.
///
/// The caller decides how to surface this: `nmp-ffi`'s `NmpApp::register_action`
/// logs it via `tracing::error!`; a test harness can `assert!(result.is_ok())`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistrationError {
    /// The namespace both registrations claimed.
    pub namespace: &'static str,
    /// Type name of the module that was already registered (first writer).
    pub prior_provider: &'static str,
    /// Type name of the module that triggered the collision (second writer).
    pub new_provider: &'static str,
}

impl core::fmt::Display for RegistrationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "action namespace '{}' already registered by '{}'; \
             a second app registration ('{}') is a composition collision \
             (ADR-0049). Two app modules must not claim the same namespace.",
            self.namespace, self.prior_provider, self.new_provider
        )
    }
}

/// Why a synchronous [`super::ActionRegistry::execute`] leg did not cleanly
/// accept-and-enqueue an action — the failure taxonomy (#1676 BUG-B).
///
/// Before #1676, a module's intentional sync `Err(String)` and a caught panic
/// collapsed into the same opaque `Err`, indistinguishable except by
/// string-matching the `"action executor panicked"` sentinel. This enum tags a
/// crash distinctly from a refusal so hosts and diagnostics can tell them apart.
///
/// The fourth taxonomy branch the issue names — **async terminal failure** (an
/// off-thread worker sending [`crate::actor::ActorCommand::RecordActionFailure`]
/// *after* `execute` already returned `Ok`) — is intentionally NOT represented
/// here: it never flows through `execute`'s synchronous return value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionFailureKind {
    /// No module is registered under the namespace. A pre-enqueue rejection by
    /// definition — nothing ran, nothing was enqueued.
    NoExecutor,
    /// The module's `execute` returned `Err(_)` intentionally — a refusal /
    /// precondition failure discovered at execution time (not a crash).
    SyncError,
    /// The module's `execute` panicked. Contained by `catch_unwind` rather
    /// than unwinding across the FFI boundary.
    Panic,
}

/// Outcome of a failed [`super::ActionRegistry::execute`] (#1676).
///
/// Carries the failure [`kind`](Self::kind), the human-readable
/// [`message`](Self::message) the host surfaces as a toast, and the
/// load-bearing [`enqueued`](Self::enqueued) flag that enforces the
/// one-terminal-per-dispatch invariant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionExecuteFailure {
    /// Failure taxonomy (#1676 BUG-B).
    pub kind: ActionFailureKind,
    /// Human-readable failure reason.
    pub message: String,
    /// Whether the module sent at least one
    /// [`crate::actor::ActorCommand`] before failing.
    ///
    /// The execute contract (#1676 BUG-B) is **`execute` returns `Err` ⇒
    /// nothing was enqueued**, so for a well-behaved module this is always
    /// `false`. When it is `true`, the module enqueued a command that will
    /// itself produce the action's terminal verdict; the dispatch caller MUST
    /// suppress the failure fan-in, or a SECOND terminal lands under one
    /// correlation_id (#1676 BUG-A — the double-terminal bug). A sync `Err`
    /// with `enqueued == true` is a module-contract violation: loud
    /// (`debug_assert`) in dev/test, fail-safe (suppress) in release.
    pub enqueued: bool,
}
