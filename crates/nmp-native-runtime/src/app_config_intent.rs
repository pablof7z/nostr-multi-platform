//! #1804 — input-scope recognizer registration setter for `NmpApp`. A cohesive
//! sibling `impl NmpApp` block (kept out of `app_config_substrate.rs` to respect
//! the file-size hard cap). Registers a crate-owned `InputScopeRecognizer` into
//! the shared input-scope registry the runtime input-intent API reads.

use crate::{NmpApp, NmpConfigStatus};

impl NmpApp {
    /// Snapshot the registered input-scope recognizers for pure intent
    /// classification.
    #[must_use]
    pub fn input_scope_recognizers(
        &self,
    ) -> Vec<std::sync::Arc<dyn nmp_core::substrate::InputScopeRecognizer>> {
        self.composition.input_scope_registry.recognizers()
    }

    /// Register an [`nmp_core::substrate::InputScopeRecognizer`] against the
    /// shared crate-registered input-scope registry. Per-protocol / app crates
    /// call this from their composition helper (e.g. a NIP-50 text-search
    /// recognizer wired by `explicit owner composition`, or an app's own
    /// recognizer).
    ///
    /// SHOULD be called before `nmp_app_start` (composition-root house style;
    /// mirrors `register_search_scope`). A duplicate scope id **yields**
    /// (ADR-0049): the first registration keeps the scope; a later one for the
    /// same id is recorded as `YieldedToExisting` in the `"input_scope"`
    /// composition-ledger seam and is NOT installed.
    pub fn register_input_scope(
        &self,
        recognizer: std::sync::Arc<dyn nmp_core::substrate::InputScopeRecognizer>,
    ) -> NmpConfigStatus {
        let scope_label = recognizer.scope().label();
        if let Err(status) = self.ensure_prestart_config(
            nmp_core::substrate::INPUT_SCOPE_LEDGER_SEAM,
            scope_label.clone(),
            std::any::type_name::<dyn nmp_core::substrate::InputScopeRecognizer>(),
        ) {
            return status;
        }
        let disposition = self.composition.input_scope_registry.register(recognizer);
        // ADR-0049 Part 2 — record the install/yield decision in the
        // "input_scope" ledger seam.
        let ledger_disposition = match disposition {
            nmp_core::substrate::InputScopeDisposition::Installed => {
                nmp_core::Disposition::Installed
            }
            nmp_core::substrate::InputScopeDisposition::YieldedToExisting => {
                nmp_core::Disposition::YieldedToExisting
            }
        };
        self.composition_ledger.record(
            nmp_core::substrate::INPUT_SCOPE_LEDGER_SEAM,
            scope_label,
            std::any::type_name::<dyn nmp_core::substrate::InputScopeRecognizer>(),
            ledger_disposition,
            None,
        );
        NmpConfigStatus::Ok
    }
}
