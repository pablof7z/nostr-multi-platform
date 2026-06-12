//! `CompositionLedger` — the explain-the-composition surface (ADR-0049 Part 2).
//!
//! # Why this exists
//!
//! D6 makes NMP's composition **silent by design**: a registration that yields,
//! replaces, or is dropped late never panics across the C-ABI. Spring Boot's
//! auto-configuration proved that silent composition is only viable WITH an
//! explain surface — its `ConditionEvaluationReport` answers "which beans were
//! installed, which yielded to a user bean, and why". This ledger is NMP's
//! analog: an append-only record of every host-init registration decision,
//! readable back as JSON through `nmp_app_composition_report`.
//!
//! # What is recorded (and what is NOT)
//!
//! Recorded — exactly the seams where a composition decision is made:
//!
//! * **action registry** — every [`crate::kernel::ActionRegistry::register`] /
//!   `register_default` call, with the resolved [`Disposition`] (installed /
//!   replaced / yielded).
//! * **ingest parsers, snapshot projections** — recorded at the AppHost
//!   registration paths in `nmp-ffi`.
//! * **last-writer-wins slots** (`set_routing_substrate`, `set_coverage_hook`,
//!   `set_nostrconnect_bootstrap_relay`) — [`Disposition::ReplacedPrevious`]
//!   when overwriting an already-installed value.
//! * **dropped late wiring** — a setter invoked after `nmp_app_start`, whose
//!   value the actor will never read. This finally implements the
//!   `KernelDiagnostic::LateWiring` promise that `nmp-defaults/src/builder.rs`
//!   documented but never built.
//!
//! NOT recorded: the hot path. The ledger is written only during host-init
//! registration and the rare runtime slot replacement — never on the actor
//! tick, never on ingest, never on dispatch. It is an append-only `Vec` behind
//! a `Mutex`, read once when a host pulls the report (D8: no polling, no
//! background work).

use std::sync::Mutex;

use serde::Serialize;

/// What happened to a single registration attempt.
///
/// The four dispositions mirror the order-independent-yielding semantics of
/// ADR-0049 Part 1 plus the late-wiring drop of Part 2.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum Disposition {
    /// A first-time registration: the key/slot was unclaimed and is now held
    /// by `provider`.
    Installed,
    /// `provider` overwrote a value previously installed by another provider
    /// (app-over-default override, or a slot re-set). The previous holder is
    /// recorded in [`CompositionRecord::replaced`].
    ReplacedPrevious,
    /// A **yielding default** declined to install because the key was already
    /// claimed (by an app or an earlier default). The existing holder keeps the
    /// slot; `provider` here is the default that yielded.
    YieldedToExisting,
    /// A setter was invoked after `nmp_app_start`: the actor has already read
    /// the wiring slots once at kernel construction, so this value is dropped
    /// and never takes effect (the `KernelDiagnostic::LateWiring` case).
    DroppedLateWiring,
}

impl Disposition {
    /// Lowercase, stable wire token (the JSON discriminant the host decodes).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Disposition::Installed => "installed",
            Disposition::ReplacedPrevious => "replaced_previous",
            Disposition::YieldedToExisting => "yielded_to_existing",
            Disposition::DroppedLateWiring => "dropped_late_wiring",
        }
    }
}

/// One composition decision.
///
/// `seam` is a `&'static str` naming the registration surface
/// (`"action_registry"`, `"ingest_parser"`, `"snapshot_projection"`,
/// `"routing_substrate"`, `"coverage_hook"`, `"nostrconnect_bootstrap_relay"`,
/// …). `key` is the seam-local identity (an action namespace, a kind, a
/// projection key, or the slot name when a slot is singular). `provider` is the
/// registering module/crate — typically `std::any::type_name::<M>()`.
#[derive(Clone, Debug, Serialize)]
pub struct CompositionRecord {
    pub seam: &'static str,
    pub key: String,
    pub provider: String,
    pub disposition: Disposition,
    /// The provider previously holding `key`, set only for
    /// [`Disposition::ReplacedPrevious`] / [`Disposition::YieldedToExisting`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced: Option<String>,
}

/// Schema version for the `nmp_app_composition_report` JSON payload. Bump on
/// any breaking shape change so a host decoder can branch.
pub const COMPOSITION_REPORT_SCHEMA_VERSION: u32 = 1;

/// Append-only ledger of composition decisions.
///
/// Cheap to clone the handle (`Arc<CompositionLedger>`); the records live
/// behind a single `Mutex<Vec<…>>`. Every recording method takes `&self` so a
/// shared `Arc` can be handed to both the action registry and the FFI slot
/// setters without `&mut`.
///
/// D6 — a poisoned lock makes recording a silent no-op and the report an empty
/// document; a ledger failure never crashes the host or aborts a registration.
#[derive(Debug, Default)]
pub struct CompositionLedger {
    records: Mutex<Vec<CompositionRecord>>,
}

impl CompositionLedger {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one record. Silent no-op on a poisoned lock (D6).
    pub fn record(
        &self,
        seam: &'static str,
        key: impl Into<String>,
        provider: impl Into<String>,
        disposition: Disposition,
        replaced: Option<String>,
    ) {
        if let Ok(mut records) = self.records.lock() {
            records.push(CompositionRecord {
                seam,
                key: key.into(),
                provider: provider.into(),
                disposition,
                replaced,
            });
        }
    }

    /// Snapshot the ledger as the canonical report JSON value.
    ///
    /// Shape (stable, schema-versioned):
    ///
    /// ```text
    /// {
    ///   "schema_version": 1,
    ///   "count": 7,
    ///   "records": [
    ///     { "seam": "action_registry", "key": "nmp.nip02.follow",
    ///       "provider": "nmp_nip02::FollowModule", "disposition": "Installed" },
    ///     { "seam": "action_registry", "key": "nmp.publish",
    ///       "provider": "app::MyPublish", "disposition": "ReplacedPrevious",
    ///       "replaced": "nmp_core::publish::PublishModule" },
    ///     ...
    ///   ]
    /// }
    /// ```
    ///
    /// A poisoned lock yields an empty (`count: 0`) document rather than failing
    /// (D6) — the host's decoder never branches on null-vs-empty.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let records = self
            .records
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        serde_json::json!({
            "schema_version": COMPOSITION_REPORT_SCHEMA_VERSION,
            "count": records.len(),
            "records": records,
        })
    }

    /// Test-only: number of records currently held.
    #[cfg(test)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.lock().map(|r| r.len()).unwrap_or(0)
    }

    /// Test-only: clone of all records (for assertions).
    #[cfg(test)]
    #[must_use]
    pub fn records(&self) -> Vec<CompositionRecord> {
        self.records.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "composition_ledger/tests.rs"]
mod tests;
