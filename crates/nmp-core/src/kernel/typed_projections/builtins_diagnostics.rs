//! Wave C action-lifecycle + relay-diagnostics-cluster slice of
//! [`Kernel::builtin_typed_projections`].
//!
//! These FIVE built-ins (`action_results` / `signed_events` / `action_stages` /
//! `action_lifecycle` / `relay_diagnostics`) differ from every prior Wave C
//! built-in: their producing accessors must NOT be invoked from the typed path,
//! because they DRAIN (`action_results` / `signed_events`), run a wall-clock TTL
//! sweep (`action_lifecycle`), or are mutated mid-tick by the `action_results`
//! drain (`action_stages`). `relay_diagnostics` is the cheap-but-non-trivial
//! roll-up of the full relay/wire-sub tree; it is captured once for the same
//! divergence-safety reason (the JSON and typed forms must read the SAME struct
//! in the SAME tick) and to avoid rebuilding the whole tree twice per tick. So
//! each is CAPTURED once at its JSON-insertion site in
//! [`snapshot_projections_with_publish_cluster`](super::super::Kernel::snapshot_projections_with_publish_cluster)
//! into a per-tick `Kernel` field, and this cluster encodes the typed sidecar
//! from that captured value — guaranteeing the JSON and typed forms read the SAME
//! data in the SAME tick (the divergence-safety invariant).
//!
//! Conditionality (mirrors the JSON insertion exactly):
//! - the four drain-on-emit built-ins are pushed ONLY when their capture is
//!   `Some` — i.e. exactly when the JSON key was inserted this tick. They are
//!   ABSENT in steady state (nothing settled / tracked → nothing captured).
//! - `relay_diagnostics` is UNCONDITIONAL: its JSON key is always inserted, and
//!   `snapshot_projections_with_publish_cluster` always captures the struct
//!   before the typed path runs, so its typed entry is always present in an
//!   emitted frame (including a fresh kernel, with empty `relays`/`interests`).
//!
//! The four drained accessors expose only `serde_json::Value` (the DTO is gone
//! after the drain), so their Models are built by PARSING the captured `Value`
//! (their codecs' `model_from_json`). `relay_diagnostics` survives as a real
//! struct, so it uses the #1031 struct->Model mapping here (the `pub(super)`
//! struct fields are reachable from this `kernel::` descendant).

use super::{
    encode_action_lifecycle, encode_action_results, encode_action_stages, encode_relay_diagnostics,
    encode_signed_events, ActionLifecycleModel, ActionResultsModel, ActionStagesModel,
    InfoRow, InterestRow, RelayDiagnosticsModel, RelayRow, SignedEventsModel, WireSubRow,
    ACTION_LIFECYCLE_FILE_IDENTIFIER, ACTION_LIFECYCLE_SCHEMA_ID, ACTION_LIFECYCLE_SCHEMA_VERSION,
    ACTION_RESULTS_FILE_IDENTIFIER, ACTION_RESULTS_SCHEMA_ID, ACTION_RESULTS_SCHEMA_VERSION,
    ACTION_STAGES_FILE_IDENTIFIER, ACTION_STAGES_SCHEMA_ID, ACTION_STAGES_SCHEMA_VERSION,
    RELAY_DIAGNOSTICS_FILE_IDENTIFIER, RELAY_DIAGNOSTICS_SCHEMA_ID, RELAY_DIAGNOSTICS_SCHEMA_VERSION,
    SIGNED_EVENTS_FILE_IDENTIFIER, SIGNED_EVENTS_SCHEMA_ID, SIGNED_EVENTS_SCHEMA_VERSION,
};
use crate::update_envelope::TypedProjectionData;

/// Map one captured `RelayDiagnosticsWireSub` struct onto the codec's
/// [`WireSubRow`]. The `pub(super)` struct fields are reachable here (a
/// `kernel::` descendant); the compiler checks completeness of the struct read.
fn wire_sub_row(sub: &super::super::relay_diagnostics::RelayDiagnosticsWireSub) -> WireSubRow {
    WireSubRow {
        wire_id: sub.wire_id.clone(),
        short_wire_id: sub.short_wire_id.clone(),
        relay_url: sub.relay_url.clone(),
        filter_summary: sub.filter_summary.clone(),
        state_label: sub.state_label.clone(),
        state_tone: sub.state_tone.clone(),
        consumer_count_label: sub.consumer_count_label.clone(),
        events_rx_display: sub.events_rx_display.clone(),
        eose_observed: sub.eose_observed,
        opened_ms: sub.opened_ms,
        last_event_ms: sub.last_event_ms.unwrap_or(0),
        eose_ms: sub.eose_ms.unwrap_or(0),
        close_reason: sub.close_reason.clone(),
    }
}

/// Map one captured `RelayDiagnosticsRow` struct onto the codec's [`RelayRow`].
fn relay_row(row: &super::super::relay_diagnostics::RelayDiagnosticsRow) -> RelayRow {
    RelayRow {
        relay_url: row.relay_url.clone(),
        short_url: row.short_url.clone(),
        role_label: row.role_label.clone(),
        role_tone: row.role_tone.clone(),
        connection_label: row.connection_label.clone(),
        connection_tone: row.connection_tone.clone(),
        auth_label: row.auth_label.clone(),
        auth_tone: row.auth_tone.clone(),
        total_sub_count: row.total_sub_count,
        active_sub_count: row.active_sub_count,
        eosed_sub_count: row.eosed_sub_count,
        total_events_rx: row.total_events_rx,
        total_events_display: row.total_events_display.clone(),
        reconnect_count: row.reconnect_count,
        bytes_rx_display: row.bytes_rx_display.clone(),
        bytes_tx_display: row.bytes_tx_display.clone(),
        last_connected_ms: row.last_connected_ms.unwrap_or(0),
        last_event_ms: row.last_event_ms.unwrap_or(0),
        last_notice: row.last_notice.clone(),
        last_error: row.last_error.clone(),
        wire_subs: row.wire_subs.iter().map(wire_sub_row).collect(),
        info: row.info.as_ref().map(info_row),
    }
}

/// Map one captured `RelayDiagnosticsInfo` struct onto the codec's [`InfoRow`]
/// (ADR-0051). The `pub(super)` struct fields are reachable here.
fn info_row(info: &super::super::relay_diagnostics::RelayDiagnosticsInfo) -> InfoRow {
    InfoRow {
        name: info.name.clone(),
        description: info.description.clone(),
        icon: info.icon.clone(),
        pubkey: info.pubkey.clone(),
        contact: info.contact.clone(),
        software: info.software.clone(),
        version: info.version.clone(),
        supported_nips: info.supported_nips.clone(),
        payment_required: info.payment_required,
        auth_required: info.auth_required,
        restricted_writes: info.restricted_writes,
    }
}

/// Map one captured `RelayDiagnosticsInterest` struct onto the codec's
/// [`InterestRow`].
fn interest_row(row: &super::super::relay_diagnostics::RelayDiagnosticsInterest) -> InterestRow {
    InterestRow {
        key: row.key.clone(),
        state: row.state.clone(),
        state_tone: row.state_tone.clone(),
        refcount: row.refcount,
        cache_coverage: row.cache_coverage.clone(),
        relay_urls: row.relay_urls.clone(),
    }
}

impl super::super::Kernel {
    /// Encode the Wave C action-lifecycle + relay-diagnostics cluster (Tier-2)
    /// built-ins as typed FlatBuffer sidecar entries, in `action_results` →
    /// `signed_events` → `action_stages` → `action_lifecycle` →
    /// `relay_diagnostics` order. The first four are pushed ONLY when their
    /// per-tick capture is `Some` (present iff the JSON key was inserted);
    /// `relay_diagnostics` is unconditional once captured. Called by
    /// [`builtin_typed_projections`](super::super::Kernel::builtin_typed_projections);
    /// see that method's doc for the mechanism.
    pub(in crate::kernel) fn diagnostics_cluster_typed_projections(
        &self,
    ) -> Vec<TypedProjectionData> {
        let mut out = Vec::with_capacity(5);

        // `action_results` — built by PARSING the captured drained `Value`.
        // Present iff a terminal settled this tick.
        if let Some(value) = &self.captured_action_results {
            let model: ActionResultsModel = super::action_results_fb::model_from_json(value);
            out.push(TypedProjectionData {
                key: ACTION_RESULTS_SCHEMA_ID.to_string(),
                schema_id: ACTION_RESULTS_SCHEMA_ID.to_string(),
                schema_version: ACTION_RESULTS_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(ACTION_RESULTS_FILE_IDENTIFIER)
                    .into_owned(),
                payload: encode_action_results(&model),
                // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
                ..Default::default()
            });
        }

        // `signed_events` — built by PARSING the captured drained `Value`.
        // Present iff a `SignEventForReturn` settled this tick.
        if let Some(value) = &self.captured_signed_events {
            let model: SignedEventsModel = super::signed_events_fb::model_from_json(value);
            out.push(TypedProjectionData {
                key: SIGNED_EVENTS_SCHEMA_ID.to_string(),
                schema_id: SIGNED_EVENTS_SCHEMA_ID.to_string(),
                schema_version: SIGNED_EVENTS_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(SIGNED_EVENTS_FILE_IDENTIFIER)
                    .into_owned(),
                payload: encode_signed_events(&model),
                // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
                ..Default::default()
            });
        }

        // `action_stages` — built by PARSING the captured `Value`. Present iff
        // at least one correlation_id is tracked this tick.
        if let Some(value) = &self.captured_action_stages {
            let model: ActionStagesModel = super::action_stages_fb::model_from_json(value);
            out.push(TypedProjectionData {
                key: ACTION_STAGES_SCHEMA_ID.to_string(),
                schema_id: ACTION_STAGES_SCHEMA_ID.to_string(),
                schema_version: ACTION_STAGES_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(ACTION_STAGES_FILE_IDENTIFIER)
                    .into_owned(),
                payload: encode_action_stages(&model),
                // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
                ..Default::default()
            });
        }

        // `action_lifecycle` — built by PARSING the captured `Value`. Present
        // iff anything is tracked this tick.
        if let Some(value) = &self.captured_action_lifecycle {
            let model: ActionLifecycleModel = super::action_lifecycle_fb::model_from_json(value);
            out.push(TypedProjectionData {
                key: ACTION_LIFECYCLE_SCHEMA_ID.to_string(),
                schema_id: ACTION_LIFECYCLE_SCHEMA_ID.to_string(),
                schema_version: ACTION_LIFECYCLE_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(ACTION_LIFECYCLE_FILE_IDENTIFIER)
                    .into_owned(),
                payload: encode_action_lifecycle(&model),
                // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
                ..Default::default()
            });
        }

        // `relay_diagnostics` — mapped from the captured STRUCT (#1031 path).
        // Unconditional: the JSON key is always inserted, and the struct is
        // always captured before the typed path runs.
        if let Some(snapshot) = &self.captured_relay_diagnostics {
            let model = RelayDiagnosticsModel {
                relays: snapshot.relays.iter().map(relay_row).collect(),
                interests: snapshot.interests.iter().map(interest_row).collect(),
            };
            out.push(TypedProjectionData {
                key: RELAY_DIAGNOSTICS_SCHEMA_ID.to_string(),
                schema_id: RELAY_DIAGNOSTICS_SCHEMA_ID.to_string(),
                schema_version: RELAY_DIAGNOSTICS_SCHEMA_VERSION,
                file_identifier: String::from_utf8_lossy(RELAY_DIAGNOSTICS_FILE_IDENTIFIER)
                    .into_owned(),
                payload: encode_relay_diagnostics(&model),
                // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
                ..Default::default()
            });
        }

        out
    }
}
