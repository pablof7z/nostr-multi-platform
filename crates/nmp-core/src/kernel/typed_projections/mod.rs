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

mod builtins_publish;
mod configured_relays_fb;
mod outbox_summary_fb;
mod publish_outbox_fb;
mod publish_queue_fb;
mod relay_role_options_fb;
mod settings_hub_fb;

pub(crate) use configured_relays_fb::{
    encode_configured_relays, ConfiguredRelaysModel, CONFIGURED_RELAYS_FILE_IDENTIFIER,
    CONFIGURED_RELAYS_SCHEMA_ID, CONFIGURED_RELAYS_SCHEMA_VERSION,
};
// `RelayRoleOptionRow` is named in the inline mapping in
// `builtin_typed_projections` below; `ConfiguredRelayRow` is named only inside
// its own codec module + tests (so it is not re-exported here).
pub(crate) use relay_role_options_fb::{
    encode_relay_role_options, RelayRoleOptionRow, RelayRoleOptionsModel,
    RELAY_ROLE_OPTIONS_FILE_IDENTIFIER, RELAY_ROLE_OPTIONS_SCHEMA_ID,
    RELAY_ROLE_OPTIONS_SCHEMA_VERSION,
};
pub(crate) use settings_hub_fb::{
    encode_settings_hub, SettingsHubModel, SETTINGS_HUB_FILE_IDENTIFIER, SETTINGS_HUB_SCHEMA_ID,
    SETTINGS_HUB_SCHEMA_VERSION,
};
// Wave C publish/outbox cluster. The nested-row types (`PublishQueueEntryRow`,
// `RelayAckOutcomeRow`, `PublishOutboxItemRow`, `PublishOutboxRelayRow`) are
// named in the inline mappings in `builtin_typed_projections` below — where the
// `pub(super)`/`pub(crate)` DTO types are reachable — so they are re-exported
// here alongside their `Model` + encode entry points.
pub(crate) use outbox_summary_fb::{
    encode_outbox_summary, OutboxSummaryModel, OUTBOX_SUMMARY_FILE_IDENTIFIER,
    OUTBOX_SUMMARY_SCHEMA_ID, OUTBOX_SUMMARY_SCHEMA_VERSION,
};
pub(crate) use publish_outbox_fb::{
    encode_publish_outbox, PublishOutboxItemRow, PublishOutboxModel, PublishOutboxRelayRow,
    PUBLISH_OUTBOX_FILE_IDENTIFIER, PUBLISH_OUTBOX_SCHEMA_ID, PUBLISH_OUTBOX_SCHEMA_VERSION,
};
pub(crate) use publish_queue_fb::{
    encode_publish_queue, PublishQueueEntryRow, PublishQueueModel, RelayAckOutcomeRow,
    PUBLISH_QUEUE_FILE_IDENTIFIER, PUBLISH_QUEUE_SCHEMA_ID, PUBLISH_QUEUE_SCHEMA_VERSION,
};

#[cfg(test)]
pub(crate) use configured_relays_fb::decode_configured_relays;
#[cfg(test)]
pub(crate) use outbox_summary_fb::decode_outbox_summary;
#[cfg(test)]
pub(crate) use publish_outbox_fb::decode_publish_outbox;
#[cfg(test)]
pub(crate) use publish_queue_fb::decode_publish_queue;
#[cfg(test)]
pub(crate) use relay_role_options_fb::decode_relay_role_options;
#[cfg(test)]
pub(crate) use settings_hub_fb::decode_settings_hub;

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
        let mut out = Vec::with_capacity(6);

        // `configured_relays` — encoded from the SAME `AppRelay` slice the JSON
        // path serialises (`configured_relays_snapshot()`).
        let configured_relays: ConfiguredRelaysModel =
            self.configured_relays_snapshot().into();
        out.push(TypedProjectionData {
            key: CONFIGURED_RELAYS_SCHEMA_ID.to_string(),
            schema_id: CONFIGURED_RELAYS_SCHEMA_ID.to_string(),
            schema_version: CONFIGURED_RELAYS_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(CONFIGURED_RELAYS_FILE_IDENTIFIER)
                .into_owned(),
            payload: encode_configured_relays(&configured_relays),
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
        });

        // Wave C publish cluster (`publish_queue` / `publish_outbox` /
        // `outbox_summary`). Extracted to `builtins_publish.rs` to keep this
        // file under the LOC ceiling: the DTO→Row mappings are heavier (nested
        // rows) and must be inlined where the `pub(super)`/`pub(crate)` DTO
        // types are reachable, but they stay under the same owner.
        out.extend(self.publish_cluster_typed_projections());

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
