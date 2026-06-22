//! #1811 — FTS search-scope registration setter for `NmpApp`. A cohesive
//! sibling `impl NmpApp` block (extracted from `app_config_substrate.rs` to
//! keep that file under the file-size hard cap). Registers a crate-owned
//! `SearchScopeProvider` into the shared scope registry that the actor kernel
//! compiles + installs into the store at construction.

use crate::{NmpApp, NmpConfigStatus};

impl NmpApp {
    /// Register a [`nmp_core::substrate::SearchScopeProvider`] against the shared
    /// crate-registered FTS scope registry. Per-protocol crates call this from
    /// their composition helper — e.g. `nmp_nip50::register_search_scopes`
    /// (wired by `nmp_defaults::register_defaults`) and
    /// `nmp_nip29::register_search_scopes` (leaf-app opt-in for group search).
    ///
    /// MUST be called before `nmp_app_start` so the registry is compiled +
    /// installed into the kernel store at construction. A duplicate scope id
    /// **yields** (ADR-0049): the first registration keeps the scope; a later
    /// one for the same id is recorded as `YieldedToExisting` in the
    /// `"search_scope"` composition-ledger seam and is NOT installed.
    pub fn register_search_scope(
        &self,
        provider: std::sync::Arc<dyn nmp_core::substrate::SearchScopeProvider>,
    ) -> NmpConfigStatus {
        let scope_label = provider.spec().scope.label();
        if let Err(status) = self.ensure_prestart_config(
            "search_scope",
            scope_label,
            std::any::type_name::<dyn nmp_core::substrate::SearchScopeProvider>(),
        ) {
            return status;
        }
        let disposition = self.search_scope_registry.register(provider);
        // ADR-0049 Part 2 — record the install/yield decision in the
        // "search_scope" ledger seam.
        let ledger_disposition = match disposition {
            nmp_core::substrate::SearchScopeDisposition::Installed => {
                nmp_core::Disposition::Installed
            }
            nmp_core::substrate::SearchScopeDisposition::YieldedToExisting => {
                nmp_core::Disposition::YieldedToExisting
            }
        };
        self.composition_ledger.record(
            "search_scope",
            scope_label,
            std::any::type_name::<dyn nmp_core::substrate::SearchScopeProvider>(),
            ledger_disposition,
            None,
        );
        NmpConfigStatus::Ok
    }
}
