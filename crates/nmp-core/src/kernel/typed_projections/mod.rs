//! Tier-2 typed-projection codecs — the kernel-owned built-in counterpart to
//! the host-registered Tier-1 typed projections (ADR-0037).
//!
//! ## The two tiers
//!
//! ADR-0037 carries a strongly-typed FlatBuffer for each snapshot projection in
//! the `SnapshotFrame`'s `typed_projections` sidecar, ALONGSIDE the generic
//! `serde_json::Value` subtree, under the SAME key. A host with a decoder for a
//! key prefers the typed payload; an un-updated host falls back to the generic
//! `Value`.
//!
//! - **Tier-1** (protocol/app crates, e.g. `nmp-nip17`): projections a host
//!   registers via `SnapshotRegistry::register_typed`. Their typed closure reads
//!   host state parked behind a shared `Arc<Mutex>` slot and is collected by
//!   `Kernel::run_typed_projections()`.
//! - **Tier-2** (this module): projections the kernel owns and inserts directly
//!   into `KernelSnapshot::projections` inside
//!   [`Kernel::snapshot_projections_with_publish_cluster`]. They read live
//!   `&self` kernel state, so they **cannot** be expressed as a no-arg
//!   `register_typed` closure (that closure has no access to `&self` — only to
//!   shared slots). The same constraint the built-in JSON projections already
//!   carry: see the doc comment on
//!   `snapshot_projections_with_publish_cluster` —
//!   *"these are kernel-owned, so they cannot be expressed as a
//!   `SnapshotRegistry` closure — they are inserted here directly"*.
//!
//! ## The Tier-2 mechanism (the Wave C template)
//!
//! Direct emission. [`Kernel::builtin_typed_projections`] is a pure
//! `fn(&self) -> Vec<TypedProjectionData>` that encodes one entry per
//! kernel-owned projection from the SAME accessor outputs the JSON insertion in
//! `snapshot_projections_with_publish_cluster` reads. `make_update` appends its
//! result to the host-registered `run_typed_projections()` vector before
//! encoding the frame, so both representations ride the same sidecar. Sharing
//! the accessor (not a parallel struct) is what guarantees the JSON and typed
//! forms cannot structurally diverge.
//!
//! Adding the next of the ~20 built-ins is one new codec module here plus one
//! `push` in `builtin_typed_projections`. No registry plumbing, no shared slot,
//! no mirrored state.
//!
//! ## Doctrine
//!
//! - **D0**: these are kernel-owned *framework* projections. Relay configuration
//!   (`configured_relays` / `relay_role_options`) and the relay-count settings
//!   summary (`settings_hub`) are generic transport/settings primitives, not app
//!   nouns — they carry no protocol-specific (NIP-NN) semantics. The Wave C
//!   publish cluster (`publish_queue` / `publish_outbox` / `outbox_summary`) is
//!   likewise generic: it is the in-flight + settled state of the kernel's
//!   store-and-forward publish pipeline — a framework transport noun. Event
//!   *kinds* appear only as opaque `uint` passthroughs (the kernel pre-formats
//!   every kind-dependent label/icon string), so no NIP semantics leak into the
//!   shell.
//! - **D5**: each buffer is screen-shaped (the exact shape a settings or outbox
//!   screen binds), bounded by the configured relay set / the in-flight publish
//!   set — no unbounded fan-out.
//! - **D6**: every `decode_*` returns `Err(String)` on malformed input; no panic
//!   at the boundary.

mod accounts_fb;
mod active_account_fb;
// V-112 (ADR-0042): author_view_fb / thread_view_fb deleted.
mod builtins_publish;
mod builtins_views;
mod configured_relays_fb;
mod outbox_summary_fb;
mod profile_fb;
mod publish_outbox_fb;
mod publish_queue_fb;
mod relay_role_options_fb;
mod settings_hub_fb;
// Wave C profile/event cluster (appended; see `builtins_profiles.rs`).
mod builtins_profiles;
mod claimed_events_fb;
mod claimed_profiles_fb;
mod mention_profiles_fb;
mod resolved_profiles_fb;
// Wave C action-lifecycle + relay-diagnostics cluster (appended; see
// `builtins_diagnostics.rs`). These five are capture-once built-ins — their
// producing accessors drain / mutate / format-against-now, so the typed path
// reads a per-tick `Kernel`-field capture written at the JSON-insertion site.
mod action_lifecycle_fb;
mod action_results_fb;
mod action_stages_fb;
mod builtins_diagnostics;
mod relay_diagnostics_fb;
mod signed_events_fb;

pub use configured_relays_fb::{
    ConfiguredRelayRow, ConfiguredRelaysModel, CONFIGURED_RELAYS_FILE_IDENTIFIER,
    CONFIGURED_RELAYS_SCHEMA_ID, CONFIGURED_RELAYS_SCHEMA_VERSION,
};
pub(crate) use configured_relays_fb::encode_configured_relays;
// `RelayRoleOptionRow` is named in the inline mapping in
// `builtin_typed_projections` below; `ConfiguredRelayRow` is named only inside
// its own codec module + tests (so it is not re-exported here).
pub(crate) use relay_role_options_fb::{
    encode_relay_role_options, RelayRoleOptionRow, RelayRoleOptionsModel,
    RELAY_ROLE_OPTIONS_FILE_IDENTIFIER, RELAY_ROLE_OPTIONS_SCHEMA_ID,
    RELAY_ROLE_OPTIONS_SCHEMA_VERSION,
};
pub use settings_hub_fb::{
    SettingsHubModel, SETTINGS_HUB_FILE_IDENTIFIER, SETTINGS_HUB_SCHEMA_ID,
    SETTINGS_HUB_SCHEMA_VERSION,
};
pub(crate) use settings_hub_fb::encode_settings_hub;
// Wave C publish/outbox cluster. The nested-row types (`PublishQueueEntryRow`,
// `RelayAckOutcomeRow`, `PublishOutboxItemRow`, `PublishOutboxRelayRow`) are
// named in the inline mappings in `builtin_typed_projections` below — where the
// `pub(super)`/`pub(crate)` DTO types are reachable — so they are re-exported
// here alongside their `Model` + encode entry points.
pub use outbox_summary_fb::{
    OutboxSummaryModel, OUTBOX_SUMMARY_FILE_IDENTIFIER, OUTBOX_SUMMARY_SCHEMA_ID,
    OUTBOX_SUMMARY_SCHEMA_VERSION,
};
pub(crate) use outbox_summary_fb::encode_outbox_summary;
pub use publish_outbox_fb::{
    PublishOutboxItemRow, PublishOutboxModel, PublishOutboxRelayRow,
    PUBLISH_OUTBOX_FILE_IDENTIFIER, PUBLISH_OUTBOX_SCHEMA_ID, PUBLISH_OUTBOX_SCHEMA_VERSION,
};
pub(crate) use publish_outbox_fb::encode_publish_outbox;
// Internal-only encoder; the publicly re-exported `publish_queue` names
// (`PublishQueueModel` / `PublishQueueEntryRow` / `RelayAckOutcomeRow` / the
// envelope constants) live in the PUBLIC block below so they are not declared
// twice in this module's namespace.
pub(crate) use publish_queue_fb::encode_publish_queue;
// Wave C identity + views cluster (`accounts` / `active_account` / `profile`).
// V-112 (ADR-0042): `author_view` / `thread_view` FlatBuffer codecs deleted.
pub use accounts_fb::{
    AccountSummaryRow, AccountsModel, ACCOUNTS_FILE_IDENTIFIER, ACCOUNTS_SCHEMA_ID,
    ACCOUNTS_SCHEMA_VERSION,
};
pub(crate) use accounts_fb::encode_accounts;
pub use active_account_fb::{
    ActiveAccountModel, ACTIVE_ACCOUNT_FILE_IDENTIFIER, ACTIVE_ACCOUNT_SCHEMA_ID,
    ACTIVE_ACCOUNT_SCHEMA_VERSION,
};
pub(crate) use active_account_fb::encode_active_account;
pub use profile_fb::{
    ProfileCardModel, PROFILE_FILE_IDENTIFIER, PROFILE_SCHEMA_ID, PROFILE_SCHEMA_VERSION,
};
pub(crate) use profile_fb::encode_profile;
// Wave C profile/event cluster (`mention_profiles` / `claimed_profiles` /
// `claimed_events` / `resolved_profiles`). The map-entry / row types
// (`MentionProfileRow`, `ClaimedEventRow`) and the shared `ProfileCardModel`
// (from `profile_fb`, reused by `claimed_profiles` / `resolved_profiles`) are
// named in the inline mappings in `builtins_profiles.rs`, so they are
// re-exported here alongside their `Model` + encode entry points.
pub use claimed_events_fb::{
    ClaimedEventRow, ClaimedEventsModel, CLAIMED_EVENTS_FILE_IDENTIFIER,
    CLAIMED_EVENTS_SCHEMA_ID, CLAIMED_EVENTS_SCHEMA_VERSION,
};
pub(crate) use claimed_events_fb::encode_claimed_events;
pub use claimed_profiles_fb::{
    ClaimedProfilesModel, CLAIMED_PROFILES_FILE_IDENTIFIER, CLAIMED_PROFILES_SCHEMA_ID,
    CLAIMED_PROFILES_SCHEMA_VERSION,
};
pub(crate) use claimed_profiles_fb::encode_claimed_profiles;
pub(crate) use mention_profiles_fb::{
    encode_mention_profiles, MentionProfileRow, MentionProfilesModel,
    MENTION_PROFILES_FILE_IDENTIFIER, MENTION_PROFILES_SCHEMA_ID, MENTION_PROFILES_SCHEMA_VERSION,
};
pub use resolved_profiles_fb::{
    ResolvedProfilesModel, RESOLVED_PROFILES_FILE_IDENTIFIER, RESOLVED_PROFILES_SCHEMA_ID,
    RESOLVED_PROFILES_SCHEMA_VERSION,
};
pub(crate) use resolved_profiles_fb::encode_resolved_profiles;
// Wave C action-lifecycle + relay-diagnostics cluster (`action_results` /
// `signed_events` / `action_stages` / `action_lifecycle` / `relay_diagnostics`).
// The codec Models + row types are named in the cluster's struct->Model /
// parse->Model mappings (`builtins_diagnostics.rs`), so they are re-exported here
// alongside their `Model` + encode entry points + envelope constants. The four
// drained codecs' `model_from_json` parsers are called module-qualified
// (`super::<mod>::model_from_json`) and so are not re-exported.
pub(crate) use action_lifecycle_fb::{
    encode_action_lifecycle, ActionLifecycleModel, ACTION_LIFECYCLE_FILE_IDENTIFIER,
    ACTION_LIFECYCLE_SCHEMA_ID, ACTION_LIFECYCLE_SCHEMA_VERSION,
};
// Internal-only encoder; the publicly re-exported `action_results` names
// (`ActionResultsModel` + the envelope constants) live in the PUBLIC block
// below so they are not declared twice in this module's namespace.
pub(crate) use action_results_fb::encode_action_results;
// The types from action_stages_fb and relay_diagnostics_fb are promoted to
// `pub` in the PUBLIC block below so they are accessible out-of-crate.
// Only the internal encoder symbols stay as `pub(crate)`.
pub(crate) use action_stages_fb::encode_action_stages;
pub(crate) use relay_diagnostics_fb::encode_relay_diagnostics;
pub(crate) use signed_events_fb::encode_signed_events;
// PR-B final: the signed_events decode surface is promoted to `pub` so
// out-of-crate consumers (e.g. nmp-ffi's sign_event_for_return tests) can
// read the typed sidecar instead of the deleted JSON payload.
pub use signed_events_fb::{
    decode_signed_events, SignedEventRow, SignedEventsModel, SIGNED_EVENTS_FILE_IDENTIFIER,
    SIGNED_EVENTS_SCHEMA_ID, SIGNED_EVENTS_SCHEMA_VERSION,
};

#[cfg(test)]
pub(crate) use relay_role_options_fb::decode_relay_role_options;
// Wave C profile/event cluster — `decode_claimed_events` promoted to unconditional
// pub (nmp-gallery typed-sidecar migration); `decode_claimed_profiles` promoted
// to unconditional pub (V-112 follow-up: the claimed_profiles sidecar is the
// direct observable of `claim_profile`, read out-of-tree via
// `nmp_core::typed_projections` — see the app-template `validate_claim_profile`
// example); `decode_mention_profiles` remains test-only.
pub use claimed_events_fb::decode_claimed_events;
pub use claimed_profiles_fb::decode_claimed_profiles;
#[cfg(test)]
pub(crate) use mention_profiles_fb::decode_mention_profiles;
pub use resolved_profiles_fb::decode_resolved_profiles;
// Wave C action-lifecycle + relay-diagnostics cluster — action_lifecycle
// remains test-only; action_stages, relay_diagnostics, and signed_events are
// public (promoted for the typed-first migration, PR-B).
#[cfg(test)]
pub(crate) use action_lifecycle_fb::decode_action_lifecycle;

// --- PUBLIC typed-projection decode surface --------------------------------
//
// The reachable, out-of-tree Rust API (re-exported through `kernel/mod.rs` ->
// `lib.rs` as `nmp_core::typed_projections`). External consumers (e.g.
// `tenex-off`, chirp-tui, chirp-desktop) decode the typed sidecar instead of
// string-keying the generic JSON `payload`.
//
// Return-type decision: we re-export the EXISTING internal DTOs verbatim rather
// than mirroring them into a parallel public type. These structs are already
// `#[derive(Clone, Debug, Eq, PartialEq)]` field-for-field mirrors of the wire;
// a second identical type would be exactly the wrapper layer the anti-
// abstraction doctrine forbids, and would need its own `From` glue. The single
// source of truth is the codec module's DTO.
//
// Scope: the Tier-2 cluster exposes keys here by adding one `pub use` line per
// codec module (the documented extension point — no new plumbing required).
pub use action_results_fb::{
    decode_action_results, ActionResultRow, ActionResultsModel, ACTION_RESULTS_FILE_IDENTIFIER,
    ACTION_RESULTS_SCHEMA_ID, ACTION_RESULTS_SCHEMA_VERSION,
};
pub use action_stages_fb::{
    decode_action_stages, ActionStageEntryRow, ActionStagesModel, ACTION_STAGES_FILE_IDENTIFIER,
    ACTION_STAGES_SCHEMA_ID, ACTION_STAGES_SCHEMA_VERSION,
};
pub use publish_queue_fb::{
    decode_publish_queue, PublishQueueEntryRow, PublishQueueModel, RelayAckOutcomeRow,
    PUBLISH_QUEUE_FILE_IDENTIFIER, PUBLISH_QUEUE_SCHEMA_ID, PUBLISH_QUEUE_SCHEMA_VERSION,
};
pub use relay_diagnostics_fb::{
    decode_relay_diagnostics, InfoRow, InterestRow, RelayDiagnosticsModel, RelayRow, WireSubRow,
    RELAY_DIAGNOSTICS_FILE_IDENTIFIER, RELAY_DIAGNOSTICS_SCHEMA_ID,
    RELAY_DIAGNOSTICS_SCHEMA_VERSION,
};
// PR-B: newly-promoted decode functions for the identity/views/outbox cluster.
// These replace the last remaining `payload:Value` read sites in chirp-tui
// `FeatureSnapshot` and chirp-desktop `Snapshot`.
pub use accounts_fb::decode_accounts;
pub use active_account_fb::decode_active_account;
pub use configured_relays_fb::decode_configured_relays;
pub use settings_hub_fb::decode_settings_hub;
pub use profile_fb::decode_profile;
// V-112 (ADR-0042): decode_author_view / decode_thread_view deleted.
pub use publish_outbox_fb::decode_publish_outbox;
pub use outbox_summary_fb::decode_outbox_summary;

use crate::update_envelope::TypedProjectionData;

impl super::Kernel {
    /// Encode the kernel-owned (Tier-2) built-in projections as typed
    /// FlatBuffer sidecar entries — the Wave C template.
    ///
    /// One entry per built-in, each read from the SAME accessor the JSON
    /// insertion in
    /// [`snapshot_projections_with_publish_cluster`](super::Kernel::snapshot_projections_with_publish_cluster)
    /// reads, in the same tick. `make_update` appends this vector to the
    /// host-registered [`Self::run_typed_projections`] result, so both the
    /// generic `Value` projection and its typed sidecar ride the same
    /// `SnapshotFrame` under the SAME key (ADR-0037 shared keyspace).
    ///
    /// Adding the next built-in is one new codec module under
    /// `typed_projections/` plus one `push` here — no registry plumbing, no
    /// shared slot, no mirrored state.
    ///
    /// D6: pure encode, no panics, no allocations beyond the buffers; called on
    /// the actor thread inside the snapshot tick (D8: non-blocking).
    pub(in crate::kernel) fn builtin_typed_projections(&self) -> Vec<TypedProjectionData> {
        // 6 relay/settings/publish built-ins + 3 identity built-ins
        // (`accounts` / `active_account` / `profile`) + 4 profile/event built-ins
        // (`mention_profiles` / `claimed_profiles` / `claimed_events` /
        // `resolved_profiles`, all unconditional) + up to 5 action-lifecycle /
        // diagnostics built-ins (`relay_diagnostics` unconditional once captured;
        // `action_results` / `signed_events` / `action_stages` /
        // `action_lifecycle` present only when captured this tick).
        // V-112 (ADR-0042): author_view / thread_view conditional built-ins deleted.
        let mut out = Vec::with_capacity(18);

        // `configured_relays` — encoded from the SAME `AppRelay` slice the JSON
        // path serialises (`configured_relays_snapshot()`).
        let configured_relays: ConfiguredRelaysModel = self.configured_relays_snapshot().into();
        out.push(TypedProjectionData {
            key: CONFIGURED_RELAYS_SCHEMA_ID.to_string(),
            schema_id: CONFIGURED_RELAYS_SCHEMA_ID.to_string(),
            schema_version: CONFIGURED_RELAYS_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(CONFIGURED_RELAYS_FILE_IDENTIFIER)
                .into_owned(),
            payload: encode_configured_relays(&configured_relays),
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
        });

        // `relay_role_options` — encoded from the SAME `relay_role_options()`
        // vector the JSON path serialises. Mapped inline because the element
        // type (`crate::actor::RelayRoleOption`) is only nameable under the
        // codegen-schema feature; the iterator binds it by inference here.
        let relay_role_options = RelayRoleOptionsModel {
            options: crate::actor::relay_role_options()
                .iter()
                .map(|option| RelayRoleOptionRow {
                    value: option.value.clone(),
                    label: option.label.clone(),
                    tint: option.tint.clone(),
                    is_default: option.is_default,
                })
                .collect(),
        };
        out.push(TypedProjectionData {
            key: RELAY_ROLE_OPTIONS_SCHEMA_ID.to_string(),
            schema_id: RELAY_ROLE_OPTIONS_SCHEMA_ID.to_string(),
            schema_version: RELAY_ROLE_OPTIONS_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(RELAY_ROLE_OPTIONS_FILE_IDENTIFIER)
                .into_owned(),
            payload: encode_relay_role_options(&relay_role_options),
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
        });

        // `settings_hub` — encoded from the SAME relay count the JSON path reads
        // (`configured_relays_snapshot().len()`).
        let settings_hub = SettingsHubModel {
            relay_count: self.configured_relays_snapshot().len() as u32,
        };
        out.push(TypedProjectionData {
            key: SETTINGS_HUB_SCHEMA_ID.to_string(),
            schema_id: SETTINGS_HUB_SCHEMA_ID.to_string(),
            schema_version: SETTINGS_HUB_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(SETTINGS_HUB_FILE_IDENTIFIER).into_owned(),
            payload: encode_settings_hub(&settings_hub),
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
        });

        // Wave C publish cluster (`publish_queue` / `publish_outbox` /
        // `outbox_summary`). Extracted to `builtins_publish.rs` to keep this
        // file under the LOC ceiling: the DTO→Row mappings are heavier (nested
        // rows) and must be inlined where the `pub(super)`/`pub(crate)` DTO
        // types are reachable, but they stay under the same owner.
        out.extend(self.publish_cluster_typed_projections());

        // Wave C identity cluster (`accounts` / `active_account` / `profile`
        // unconditionally). V-112 (ADR-0042): `author_view` / `thread_view`
        // conditional pushes deleted. Extracted to `builtins_views.rs`.
        out.extend(self.views_cluster_typed_projections());

        // Wave C profile/event cluster (`mention_profiles` / `claimed_profiles` /
        // `claimed_events` / `resolved_profiles`, all unconditional). Extracted to
        // `builtins_profiles.rs` to keep this file under the LOC ceiling: the
        // map→sorted-vector flattening + DTO→Row mappings are inlined where the
        // `pub(super)`/`pub(crate)` DTO types are reachable, under the same owner.
        out.extend(self.profiles_cluster_typed_projections());

        // Wave C action-lifecycle + relay-diagnostics cluster (`action_results` /
        // `signed_events` / `action_stages` / `action_lifecycle` /
        // `relay_diagnostics`). Unlike every cluster above, these read per-tick
        // `Kernel`-field CAPTURES (not live accessors): their producers drain /
        // mutate / format-against-now and must not run twice in a tick. The four
        // drain-on-emit entries are pushed only when captured this tick (present
        // iff their JSON key is); `relay_diagnostics` is unconditional once
        // captured. Extracted to `builtins_diagnostics.rs` (LOC ceiling).
        //
        // ADR-0053: the capture-based diagnostics cluster already self-gates —
        // `snapshot_projections_with_publish_cluster` only sets `captured_*` for
        // declared keys, so a gated-out diagnostics built-in (notably the
        // expensive `relay_diagnostics`) was never captured and produces no
        // encode here.
        out.extend(self.diagnostics_cluster_typed_projections());

        // ADR-0053 — the final declared-set gate, mirroring the JSON path's
        // per-key `permits()` so the typed sidecar and the JSON map carry the
        // EXACT same key set (the ADR-0037 divergence-safety invariant extended
        // to the gate). The capture-based diagnostics built-ins are already
        // gated upstream (so the costly `relay_diagnostics` encode is skipped);
        // the remaining live-accessor clusters above are cheap to encode, and
        // this filter guarantees a gated-out key never ships even if a future
        // cluster forgets its own gate. Empty declared set ⇒ `permits()` is
        // always true ⇒ no filtering (no narrowing).
        let declared = self.declared_projections_snapshot();
        if declared.is_narrowing() {
            out.retain(|entry| declared.permits(&entry.key));
        }

        out
    }

    /// Merge the kernel-owned built-in typed sidecars onto the host-registered
    /// (Tier-1) ones, with **built-in keys winning on collision**.
    ///
    /// This mirrors the generic-JSON contract in
    /// [`snapshot_projections_with_publish_cluster`](super::Kernel::snapshot_projections_with_publish_cluster):
    /// a host that registers one of the kernel's reserved keys is overwritten so
    /// the kernel-owned value stays authoritative. The typed path needs an
    /// explicit drop (not just an append) because the host-side sidecar consumer
    /// matches by the FIRST entry with a given key — a colliding host entry left
    /// in the vector would shadow the built-in and silently diverge from the JSON
    /// rule. Today nothing collides with the six relay/settings/publish keys, but
    /// this is the Wave C template for ~20 more built-ins, so the contract is
    /// enforced here once.
    pub(in crate::kernel) fn merge_builtin_typed_projections(
        &self,
        host: Vec<TypedProjectionData>,
    ) -> Vec<TypedProjectionData> {
        let builtins = self.builtin_typed_projections();
        let reserved: std::collections::HashSet<&str> =
            builtins.iter().map(|entry| entry.key.as_str()).collect();
        let mut merged: Vec<TypedProjectionData> = host
            .into_iter()
            .filter(|entry| !reserved.contains(entry.key.as_str()))
            .collect();
        merged.extend(builtins);
        merged
    }
}
