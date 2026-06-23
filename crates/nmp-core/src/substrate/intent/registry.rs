//! [`InputScopeRegistry`] + the narrow [`InputScopeRegistrar`] registration
//! trait. Mirrors the FTS [`crate::substrate::search::SearchScopeRegistry`]
//! house style: shared `Arc`, yielding-default on duplicate scope id, the
//! disposition recorded by the FFI shell in the `"input_scope"` ledger seam.

use std::sync::{Arc, Mutex};

use super::recognizer::InputScopeRecognizer;

/// Composition-ledger seam name for input-scope registrations (ADR-0049 Part 2).
pub const INPUT_SCOPE_LEDGER_SEAM: &str = "input_scope";

/// Register an [`InputScopeRecognizer`] against the host. A narrow registration
/// trait (D6 / D26): a crate takes `&impl InputScopeRegistrar`, never the whole
/// `AppHost`.
pub trait InputScopeRegistrar {
    /// Register `recognizer`. Pre-start, additive; a duplicate [`super::id::InputScopeId`]
    /// yields (ADR-0049 — first registration wins; the later one is recorded as
    /// `YieldedToExisting` in the `"input_scope"` ledger seam).
    fn register_input_scope(&self, recognizer: Arc<dyn InputScopeRecognizer>);
}

/// Outcome of one [`InputScopeRegistry::register`] call (mirrors the
/// composition-ledger dispositions so the FFI shell can record it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputScopeDisposition {
    /// First registration for this scope id.
    Installed,
    /// A later registration for an already-claimed scope id — yielded
    /// (ADR-0049). The existing recognizer keeps the scope.
    YieldedToExisting,
}

/// The crate-registered input-scope recognizer registry. Lives behind a `Mutex`
/// so a shared `Arc` can be handed to the host registration surface; the
/// FFI/resolver reads a [`InputScopeRegistry::recognizers`] snapshot to drive
/// the pure `classify` pass.
#[derive(Default)]
pub struct InputScopeRegistry {
    recognizers: Mutex<Vec<Arc<dyn InputScopeRecognizer>>>,
}

impl InputScopeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a recognizer. Yields (ADR-0049) on a duplicate scope id: the
    /// first registration keeps the scope; a later one for the same id is NOT
    /// installed and returns [`InputScopeDisposition::YieldedToExisting`]. The
    /// caller (FFI shell) records the disposition in the `"input_scope"` ledger
    /// seam.
    pub fn register(
        &self,
        recognizer: Arc<dyn InputScopeRecognizer>,
    ) -> InputScopeDisposition {
        let scope = recognizer.scope();
        let Ok(mut recognizers) = self.recognizers.lock() else {
            // D6 — a poisoned lock drops the registration silently.
            return InputScopeDisposition::YieldedToExisting;
        };
        if recognizers.iter().any(|r| r.scope() == scope) {
            return InputScopeDisposition::YieldedToExisting;
        }
        recognizers.push(recognizer);
        InputScopeDisposition::Installed
    }

    /// Snapshot of the registered recognizers for the resolver. Clones the `Arc`
    /// handles (cheap) so `classify` can run without holding the lock.
    #[must_use]
    pub fn recognizers(&self) -> Vec<Arc<dyn InputScopeRecognizer>> {
        self.recognizers
            .lock()
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    /// Number of registered recognizers (diagnostics / tests).
    #[must_use]
    pub fn len(&self) -> usize {
        self.recognizers.lock().map(|r| r.len()).unwrap_or(0)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The registry is itself an [`InputScopeRegistrar`] so a composition root /
/// integration harness that holds the bare registry (without an `AppHost`) can
/// drive registration directly; the `AppHost`/FFI shell forwards to the same
/// `register` method, so ADR-0049 yield semantics are identical on both paths.
impl InputScopeRegistrar for InputScopeRegistry {
    fn register_input_scope(&self, recognizer: Arc<dyn InputScopeRecognizer>) {
        let _ = self.register(recognizer);
    }
}
