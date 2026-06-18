use super::super::{ClaimedEventDto, Kernel, MentionProfilePayload, ProfileCard};

// Canonical list of the kernel-owned (Tier-2) built-in projection keys.
// The registry-closure Tier-1 keys are NOT listed here; they are introspectable
// via the live `SnapshotRegistry`.
//
// This is the single source of truth for "which projection keys does the
// kernel itself produce". Two consumers depend on it staying exact:
//
// 1. The registry-coverage gate
//    (`nmp-app-chirp::ffi::tests::producer_completeness::every_codegen_registry_key_is_registered_at_runtime`)
//    asserts every `nmp-codegen` `SNAPSHOT_PROJECTIONS` json_key is either a
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
// This const is generated from the `nmp-codegen` projection registry
// (`swift_projections_registry::kernel_builtin_projection_keys`) by
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
        self.captured_action_results =
            (declared.permits("action_results") && !action_results.is_null())
                .then(|| action_results);

        // `signed_events` — drain-on-emit. Same pattern.
        let signed_events = self.take_signed_events_projection();
        self.captured_signed_events =
            (declared.permits("signed_events") && !signed_events.is_null())
                .then(|| signed_events);

        // `action_stages` — copy (TTL mirror). TTL sweep + Cleared machine MUST
        // run each tick; capture only when declared.
        let action_stages = self.action_stages_projection();
        self.captured_action_stages =
            (declared.permits("action_stages") && !action_stages.is_null())
                .then(|| action_stages);

        // `action_lifecycle` — copy (TTL mirror). Same pattern.
        let action_lifecycle = self.action_lifecycle_projection();
        self.captured_action_lifecycle =
            (declared.permits("action_lifecycle") && !action_lifecycle.is_null())
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
        if declared.permits("mention_profiles") {
            projections.insert(
                "mention_profiles".to_string(),
                serde_json::to_value(&self.mention_profiles())
                    .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default())),
            );
        }
        if declared.permits("claimed_profiles") {
            projections.insert(
                "claimed_profiles".to_string(),
                serde_json::to_value(&self.claimed_profiles())
                    .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default())),
            );
        }
        if declared.permits("claimed_events") {
            projections.insert(
                "claimed_events".to_string(),
                serde_json::to_value(&self.claimed_events())
                    .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default())),
            );
        }
        if declared.permits("resolved_profiles") {
            projections.insert(
                "resolved_profiles".to_string(),
                serde_json::to_value(&self.resolved_profiles())
                    .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default())),
            );
        }
        projections
    }

    /// `mention_profiles` accessor (aim.md §4.2): `pubkey ->
    /// MentionProfilePayload` for every author surfaced in ANY currently-open
    /// view.
    ///
    /// V-112 (ADR-0042): the `author_view` / `thread_view` item sources were
    /// deleted. The projection now returns an empty map. The mention_profiles
    /// projection is still emitted (not absent) to preserve the D1 contract;
    /// display name resolution for author/thread screens is delegated to the
    /// resolved_profiles (claimed_profiles) projection instead.
    ///
    /// This is the single accessor the snapshot's generic JSON `mention_profiles`
    /// projection AND its Tier-2 typed FlatBuffer sidecar both read, in the same
    /// tick, so the two wire forms cannot structurally diverge (ADR-0037).
    pub(in crate::kernel) fn mention_profiles(
        &self,
    ) -> std::collections::HashMap<String, MentionProfilePayload> {
        std::collections::HashMap::new()
    }

    /// `claimed_profiles` accessor — `pubkey -> ProfileCard` for every currently
    /// claimed UI profile (the reference-first component path). Missing kind:0
    /// data still emits a placeholder card so components can render an honest
    /// fallback immediately and refine in place when the profile arrives.
    /// BTreeMap for deterministic key ordering (snapshot diff stability).
    ///
    /// Shared accessor for the generic JSON projection and its Tier-2 typed
    /// sidecar — see [`Self::mention_profiles`] for the divergence-safety
    /// rationale.
    pub(in crate::kernel) fn claimed_profiles(
        &self,
    ) -> std::collections::BTreeMap<String, ProfileCard> {
        let mut claimed_profiles: std::collections::BTreeMap<String, ProfileCard> =
            std::collections::BTreeMap::new();
        for pubkey in self.profile_claims.keys() {
            // ADR-0032 / V-115: raw hex pubkey only; shells encode bech32
            // host-side. `to_npub` call removed.
            claimed_profiles.insert(
                pubkey.clone(),
                self.profile_card_for(pubkey, ""),
            );
        }
        claimed_profiles
    }

    /// `claimed_events` accessor — keyed by `primary_id` (hex64 event id for
    /// nevent/note URIs; `kind:pubkey:d_tag` coordinate for naddr URIs). Walks
    /// the current `event_claims` set and looks each key up against `self.events`
    /// via `lookup_for_primary_id`; missing entries are silently absent (D1
    /// best-effort). Entries carry raw event data only; author display state is
    /// resolved by profile components through `claim_profile` and the
    /// `claimed_profiles` / `resolved_profiles` projections. BTreeMap for
    /// deterministic key ordering.
    ///
    /// Shared accessor for the generic JSON projection and its Tier-2 typed
    /// sidecar — see [`Self::mention_profiles`] for the divergence-safety
    /// rationale.
    pub(in crate::kernel) fn claimed_events(
        &self,
    ) -> std::collections::BTreeMap<String, ClaimedEventDto> {
        let mut claimed_events: std::collections::BTreeMap<String, ClaimedEventDto> =
            std::collections::BTreeMap::new();
        for key in self.event_claims.keys() {
            if let Some(stored) = self.lookup_for_primary_id(key) {
                // Parse raw content → NFCT bytes via the injected content-parser
                // seam (no-op by default; web composition installs an
                // nmp-content-backed parser so claim_event renders the
                // kernel-parsed content tree).
                let content_tree_bytes = self.content_parser.parse_to_nfct_bytes(
                    &stored.content,
                    &stored.tags,
                    stored.kind,
                );
                claimed_events.insert(
                    key.clone(),
                    ClaimedEventDto::from_stored(key.clone(), &stored)
                        .with_content_tree(content_tree_bytes),
                );
            }
        }
        claimed_events
    }

    /// `resolved_profiles` accessor — the pre-merged `pubkey -> ProfileCard` map
    /// every consumer reads. Precedence: [`Self::claimed_profiles`] (highest) →
    /// [`Self::mention_profiles`] (lowest, only-if-absent). Always present as `{}` when empty
    /// (D1); BTreeMap for deterministic key ordering.
    ///
    /// Recomputes `claimed_profiles` / `mention_profiles` internally rather than
    /// sharing a cached result — the snapshot helper already calls each accessor
    /// independently, and caching across the JSON and typed call sites would
    /// reintroduce the divergence risk this split exists to remove.
    pub(in crate::kernel) fn resolved_profiles(
        &self,
    ) -> std::collections::BTreeMap<String, ProfileCard> {
        let mut resolved: std::collections::BTreeMap<String, ProfileCard> =
            std::collections::BTreeMap::new();

        // 1. claimed_profiles — highest precedence.
        for (pubkey, card) in self.claimed_profiles() {
            resolved.insert(pubkey, card);
        }

        // V-112 (ADR-0042): the author-view profile source was deleted. Profile
        // data for the author screen is now resolved via claimed_profiles
        // (claim_profile from nmp_app_chirp_open_author_feed).

        // 2. mention_profiles — only-if-absent (lowest precedence).
        for (pubkey, m) in self.mention_profiles() {
            resolved
                .entry(pubkey.clone())
                .or_insert_with(|| ProfileCard::from_mention(&pubkey, &m));
        }

        resolved
    }
}
