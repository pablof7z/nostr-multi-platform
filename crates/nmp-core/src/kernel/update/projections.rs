// ADR-0063 Lane H: MentionProfilePayload + ProfileCard + claimed_events removed
// (mention_profiles / claimed_profiles / resolved_profiles / claimed_events
// projection builders deleted).
use super::super::Kernel;

// Canonical list of the kernel-owned (Tier-2) built-in projection keys.
// The registry-closure Tier-1 keys are NOT listed here; they are introspectable
// via the live `SnapshotRegistry`.
//
// This is the single source of truth for "which projection keys does the
// kernel itself produce". Two consumers depend on it staying exact:
//
// 1. The registry-coverage gate
//    (`nmp-app-chirp::ffi::tests::producer_completeness::every_codegen_registry_key_is_registered_at_runtime`)
//    asserts every `nmp-codegen` `SNAPSHOT_PROJECTIONS` key is either a
//    runtime-registered Tier-1 closure key or a member of this list — closing
//    the #1084-class hole where a producer-side key rename ships without its
//    consumers (the codegen registry, the Swift/Kotlin bridges).
// 2. The in-crate pinning test
//    (`builtin_projection_keys_const_matches_runtime`) drives a real
//    `make_update` tick and asserts the emitted built-in keys are a subset of
//    this list AND that every unconditional key is present — so the const
//    cannot silently drift from the insertion code above.
//
// Conditional keys (`action_results` / `signed_events` / `action_stages` /
// `action_lifecycle` — drain-on-emit, present only on ticks where something
// settled) are listed too: the gate asks "can the kernel produce this key",
// not "is it present this tick".
//
// ## Codegen-derived (ADR-0053 / Workstream-E4) — not hand-maintained
//
// This const is generated from the neutral `nmp-codegen` projection contract
// (`projection_contract::kernel_builtin_projection_keys`, #1723) by
// `nmp gen builtin-keys`, so the kernel built-in key set is the single source
// of truth the codegen decoders also derive from — it cannot drift from what
// the shells decode. Regenerate with `cargo run -p nmp-codegen -- gen
// builtin-keys`; the `.github/workflows/codegen-drift.yml` gate fails any PR
// whose checked-in file is stale, and `builtin_projection_keys_const_matches_runtime`
// pins it against the kernel's actual `make_update` emission. DO NOT hand-edit
// the generated file.
include!("builtin_projection_keys.generated.rs");

impl Kernel {
    /// Drain the per-tick drain-on-emit projections and capture their values for
    /// the typed FlatBuffers sidecar (`diagnostics_cluster_typed_projections`).
    ///
    /// Must be called once per emit tick, BEFORE `merge_builtin_typed_projections`,
    /// so the `captured_*` fields are fresh when the typed path reads them.
    ///
    /// The drain methods (`take_action_results_projection`,
    /// `take_signed_events_projection`) also drive the ADR-0055 Rung-1
    /// `note_drain_emit` state machine — they MUST run each tick so the rev
    /// tracker sees the correct `Changed` / `Cleared` / `Unchanged` transitions.
    /// The copy-based projections (`action_stages`, `action_lifecycle`) run their
    /// TTL sweep and `note_copy_emit` machine here as well.
    pub(in crate::kernel) fn drain_and_capture_projections(&mut self) {
        let declared = self.declared_projections_snapshot();

        // `action_results` — drain-on-emit. Always drain (keeps the source
        // bounded); capture only when declared (ADR-0053).
        let action_results = self.take_action_results_projection();
        self.captured_action_results = (declared.permits("action_results")
            && !action_results.is_null())
        .then(|| action_results);

        // `signed_events` — drain-on-emit. Same pattern.
        let signed_events = self.take_signed_events_projection();
        self.captured_signed_events =
            (declared.permits("signed_events") && !signed_events.is_null()).then(|| signed_events);

        // `action_stages` — copy (TTL mirror). TTL sweep + Cleared machine MUST
        // run each tick; capture only when declared.
        let action_stages = self.action_stages_projection();
        self.captured_action_stages =
            (declared.permits("action_stages") && !action_stages.is_null()).then(|| action_stages);

        // `action_lifecycle` — copy (TTL mirror). Same pattern.
        let action_lifecycle = self.action_lifecycle_projection();
        self.captured_action_lifecycle = (declared.permits("action_lifecycle")
            && !action_lifecycle.is_null())
        .then(|| action_lifecycle);

        // `relay_diagnostics` — unconditional snapshot once declared.
        // ADR-0053: the whole roll-up is skipped when undeclared.
        if declared.permits("relay_diagnostics") {
            self.captured_relay_diagnostics = Some(self.relay_diagnostics_snapshot());
        } else {
            self.captured_relay_diagnostics = None;
        }
    }

    /// Build the JSON `projections` map for a single emit tick.
    ///
    /// Assembles the kernel-owned Tier-2 built-in projections into one
    /// `HashMap<String, serde_json::Value>`.
    ///
    /// **Callers must call `drain_and_capture_projections()` BEFORE this method**
    /// so the drain-based entries read from `captured_*` fields without a
    /// second drain.
    ///
    /// D0: no `KernelSnapshot.projections` field — this map is assembled
    /// transiently on each emit. The typed FlatBuffers sidecar is the production
    /// wire path; this map is used by test helpers that read JSON.
    #[cfg(any(test, feature = "test-support"))]
    pub(in crate::kernel) fn build_projections_map(
        &mut self,
    ) -> std::collections::HashMap<String, serde_json::Value> {
        let mut projections = std::collections::HashMap::new();
        let declared = self.declared_projections_snapshot();

        if declared.permits("publish_queue") {
            projections.insert(
                "publish_queue".to_string(),
                serde_json::to_value(self.publish_queue_snapshot())
                    .unwrap_or(serde_json::Value::Null),
            );
        }
        if declared.permits("publish_outbox") {
            projections.insert(
                "publish_outbox".to_string(),
                serde_json::to_value(self.publish_outbox_items())
                    .unwrap_or(serde_json::Value::Null),
            );
        }
        if declared.permits("outbox_summary") {
            projections.insert(
                "outbox_summary".to_string(),
                serde_json::to_value(self.outbox_summary_snapshot())
                    .unwrap_or(serde_json::Value::Null),
            );
        }
        if declared.permits("configured_relays") {
            projections.insert(
                "configured_relays".to_string(),
                serde_json::to_value(self.configured_relays_snapshot())
                    .unwrap_or(serde_json::Value::Null),
            );
        }
        if declared.permits("relay_role_options") {
            projections.insert(
                "relay_role_options".to_string(),
                serde_json::to_value(crate::actor::relay_role_options())
                    .unwrap_or(serde_json::Value::Null),
            );
        }
        if declared.permits("settings_hub") {
            projections.insert(
                "settings_hub".to_string(),
                serde_json::json!({ "relay_count": self.configured_relays_snapshot().len() }),
            );
        }
        // Drain-based projections — read from `captured_*` fields set by
        // `drain_and_capture_projections()`. Must not re-invoke draining
        // accessors here to avoid a double-drain.
        if let Some(ar) = &self.captured_action_results {
            projections.insert("action_results".to_string(), ar.clone());
        }
        if let Some(se) = &self.captured_signed_events {
            projections.insert("signed_events".to_string(), se.clone());
        }
        if let Some(as_) = &self.captured_action_stages {
            projections.insert("action_stages".to_string(), as_.clone());
        }
        if let Some(al) = &self.captured_action_lifecycle {
            projections.insert("action_lifecycle".to_string(), al.clone());
        }
        if let Some(rd) = &self.captured_relay_diagnostics {
            projections.insert(
                "relay_diagnostics".to_string(),
                serde_json::to_value(rd).unwrap_or(serde_json::Value::Null),
            );
        }
        if declared.permits("accounts") {
            let enriched = self.accounts_enriched();
            projections.insert(
                "accounts".to_string(),
                serde_json::to_value(&enriched)
                    .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
            );
        }
        if declared.permits("active_account") {
            let (_, active_account) = self.account_snapshot();
            projections.insert(
                "active_account".to_string(),
                serde_json::to_value(active_account).unwrap_or(serde_json::Value::Null),
            );
        }
        if declared.permits("profile") {
            projections.insert(
                "profile".to_string(),
                serde_json::to_value(self.profile_card()).unwrap_or(serde_json::Value::Null),
            );
        }
        // ADR-0063 Lane H: mention_profiles / claimed_profiles /
        // resolved_profiles / claimed_events emission deleted. These projections
        // are replaced by refs.profile / refs.event row-delta sidecars.
        projections
    }

    // ADR-0063 Lane H: mention_profiles(), claimed_profiles(), resolved_profiles()
    // deleted. These were the old 3-tier JSON projection builders; profile data is
    // now delivered via the refs.profile KPRF NRRD row-delta sidecar.
}
