//! ADR-0055 Rung 3 — test-only `make_update_*` helper variants.
//!
//! Extracted from `update.rs` to bring that file below the 500-LOC hard
//! ceiling and buy headroom for the Rung-3 wiring. Zero-behavior refactor:
//! these are `#[cfg(test)]` functions on `Kernel` that delegate to the same
//! shared production path as `make_update`.

use super::super::Kernel;
use crate::kernel::projection_rev;
use crate::kernel::typed_projections;
use crate::kernel::update::helpers;
use crate::kernel::update::rung2_stamp;
use crate::kernel::update::rung3_omit;
use crate::update_envelope::{decode_snapshot_typed_projections, encode_snapshot_with_envelope};

impl Kernel {
    /// PR-B (#991/#979): drive `make_update` for one tick and return BOTH the
    /// raw `UpdateFrameBytes` AND a `serde_json::Value` serialized from the same
    /// tick's `KernelSnapshot` struct (without a wire roundtrip).
    ///
    /// Used by `tier3_envelope_tests` to compare typed Tier-3 FlatBuffers fields
    /// against the struct-serialized JSON on the SAME tick — the two were
    /// previously both decoded from the same frame bytes (before payload zeroing).
    /// Now JSON comes from the struct and the typed fields come from the wire.
    /// Rev and all other fields still agree because both come from the same tick.
    pub(crate) fn make_update_frame_and_json_for_test(
        &mut self,
        running: bool,
    ) -> (crate::update_envelope::UpdateFrameBytes, serde_json::Value) {
        let emit_started = super::super::Instant::now();
        let last_tick_ms = self.now_ms();
        self.rev = self.rev.saturating_add(1);
        self.update_sequence = self.update_sequence.saturating_add(1);
        let batch_events = self.events_since_last_update;
        self.max_events_per_update = self.max_events_per_update.max(batch_events);
        let last_event_to_emit_ms = self
            .timing
            .last_event_at
            .map(|last_event_at| emit_started.duration_since(last_event_at).as_millis());
        if let Some(value) = last_event_to_emit_ms {
            self.max_event_to_emit_ms = self.max_event_to_emit_ms.max(value);
        }
        let update =
            self.build_snapshot_struct(running, last_tick_ms, emit_started, last_event_to_emit_ms);
        let before_serialize = super::super::Instant::now();
        let typed = self.run_typed_projections();
        // Drain and capture before merge so `captured_*` fields are set.
        self.drain_and_capture_projections();
        let mut typed = self.merge_builtin_typed_projections(typed);
        // Build the projections map and inject into the snapshot JSON so that
        // test assertions reading snapshot["projections"]["key"] work.
        let projections_map = self.build_projections_map();
        let mut json = serde_json::to_value(&update).unwrap_or(serde_json::Value::Null);
        if let Some(obj) = json.as_object_mut() {
            obj.insert(
                "projections".to_string(),
                serde_json::to_value(&projections_map)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
            );
        }
        let declared = self.declared_projections_snapshot();
        self.projection_rev_tracker
            .reconcile_declared_permits(&declared);
        let (incremental_enabled, baseline_pending) = self.incremental_apply_state();
        if baseline_pending {
            self.projection_rev_tracker.reset_last_emitted();
        }
        let profile_permitted = declared.permits(typed_projections::REFS_PROFILE_KEY);
        let event_permitted = declared.permits(typed_projections::REFS_EVENT_KEY);
        let mut refs_entries =
            self.refs_row_delta_projections(baseline_pending, profile_permitted, event_permitted);
        if declared.is_narrowing() {
            refs_entries.retain(|entry| declared.permits(&entry.key));
        }
        typed.extend(refs_entries);
        // ADR-0055 Rung 2: stamp rev/state/epoch identical to the production path.
        let diag_fp = helpers::diagnostics_payload_fingerprint(&typed);
        self.projection_rev_tracker
            .reconcile_diagnostics_fingerprint(diag_fp);
        let publish_engine_fp = helpers::publish_engine_payload_fingerprint(&typed);
        self.projection_rev_tracker
            .reconcile_publish_engine_fingerprint(publish_engine_fp);
        let manifest = self.projection_manifest();
        let epoch_stamp = rung2_stamp::epoch_stamp(&manifest);
        let typed = rung2_stamp::stamp_typed_projections(typed, &manifest);
        let typed = rung3_omit::omit_unchanged(typed, &manifest, incremental_enabled);
        // ADR-0055 Rung 3 (D3-6): pass the kernel-owned reusable builder,
        // matching the production path in `make_update`.
        let frame = encode_snapshot_with_envelope(
            &mut self.snapshot_builder,
            &typed,
            &update,
            &epoch_stamp,
        );
        // Advance the tracker's last-emitted baseline so the next tick's presence
        // computation is accurate (matches the production path).
        rung2_stamp::record_emitted_for_manifest(&mut self.projection_rev_tracker, &manifest);
        let this_serialize_us = before_serialize.elapsed().as_micros();
        let this_make_update_us = emit_started.elapsed().as_micros();
        self.events_since_last_update = 0;
        self.changed_since_emit = false;
        self.last_serialize_us = this_serialize_us;
        self.last_make_update_us = this_make_update_us;
        self.last_payload_bytes = frame.len();
        (frame, json)
    }

    /// PR-B (#991/#979): run a full tick (identical to `make_update`, including
    /// `run_typed_projections` and `merge_builtin_typed_projections`), then
    /// serialize the `KernelSnapshot` struct to `serde_json::Value` for test
    /// assertions.
    ///
    /// The `payload:Value` slot is no longer emitted on the wire (the decoder
    /// itself is deleted), so JSON-shaped assertions cannot come off the frame.
    /// Test helpers serialize the struct directly — equivalent coverage,
    /// no wire roundtrip.
    ///
    pub(crate) fn make_update_value_for_test(&mut self, running: bool) -> serde_json::Value {
        let emit_started = super::super::Instant::now();
        let last_tick_ms = self.now_ms();
        self.rev = self.rev.saturating_add(1);
        self.update_sequence = self.update_sequence.saturating_add(1);
        let last_event_to_emit_ms = self
            .timing
            .last_event_at
            .map(|last_event_at| emit_started.duration_since(last_event_at).as_millis());
        let snapshot =
            self.build_snapshot_struct(running, last_tick_ms, emit_started, last_event_to_emit_ms);
        let _typed_host = self.run_typed_projections();
        // Drain and capture before merge so `captured_*` fields are set.
        self.drain_and_capture_projections();
        let mut typed_merged = self.merge_builtin_typed_projections(_typed_host);
        let declared = self.declared_projections_snapshot();
        self.projection_rev_tracker
            .reconcile_declared_permits(&declared);
        let (_incremental_enabled, baseline_pending) = self.incremental_apply_state();
        if baseline_pending {
            self.projection_rev_tracker.reset_last_emitted();
        }
        let profile_permitted = declared.permits(typed_projections::REFS_PROFILE_KEY);
        let event_permitted = declared.permits(typed_projections::REFS_EVENT_KEY);
        let mut refs_entries =
            self.refs_row_delta_projections(baseline_pending, profile_permitted, event_permitted);
        if declared.is_narrowing() {
            refs_entries.retain(|entry| declared.permits(&entry.key));
        }
        typed_merged.extend(refs_entries);
        // ADR-0055 Rung 2: keep the projection-rev tracker in the same state as
        // the production path so oracle-gated tests see consistent revs.
        let diag_fp = helpers::diagnostics_payload_fingerprint(&typed_merged);
        self.projection_rev_tracker
            .reconcile_diagnostics_fingerprint(diag_fp);
        let publish_engine_fp = helpers::publish_engine_payload_fingerprint(&typed_merged);
        self.projection_rev_tracker
            .reconcile_publish_engine_fingerprint(publish_engine_fp);
        let manifest = self.projection_manifest();
        rung2_stamp::record_emitted_for_manifest(&mut self.projection_rev_tracker, &manifest);
        // Build the projections map and inject into the snapshot JSON so that
        // test assertions reading snapshot["projections"]["key"] work.
        let projections_map = self.build_projections_map();
        let mut json = serde_json::to_value(&snapshot).unwrap_or(serde_json::Value::Null);
        if let Some(obj) = json.as_object_mut() {
            obj.insert(
                "projections".to_string(),
                serde_json::to_value(&projections_map)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
            );
        }
        json
    }

    pub(crate) fn make_update_json_for_test(&mut self, running: bool) -> String {
        serde_json::to_string(&self.make_update_value_for_test(running)).unwrap_or_default()
    }

    /// Drive a single tick and return BOTH the JSON view of the `KernelSnapshot`
    /// struct AND the typed-projection sidecar from the SAME tick.
    ///
    /// Uses `make_update_frame_and_json_for_test` internally so draining
    /// projections (e.g. `action_results`, which is `take_*` / drain-on-emit)
    /// are captured in both the JSON struct serialisation and the typed sidecar
    /// in the same tick — no second tick, no double-drain.
    ///
    /// All current callers that only use the typed sidecar ignore `_value`;
    /// callers that assert BOTH channels can use both fields.
    pub(crate) fn make_update_typed_for_test(
        &mut self,
        running: bool,
    ) -> (
        serde_json::Value,
        Vec<crate::update_envelope::TypedProjectionData>,
    ) {
        let (frame, value) = self.make_update_frame_and_json_for_test(running);
        let typed = decode_snapshot_typed_projections(&frame).unwrap_or_default();
        (value, typed)
    }
}

/// ADR-0055 Rung 1 (F3) — expose the last-emitted projection state for the
/// test-support oracle (the omission-aware extension lives in `rung3_omit`
/// tests that call this to verify omitted ⟺ cache-unit unchanged).
#[cfg(any(test, feature = "test-support"))]
pub(crate) use projection_rev::build_state as build_projection_state;
