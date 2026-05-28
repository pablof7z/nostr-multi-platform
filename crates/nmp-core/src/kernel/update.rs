//! Snapshot emission: encodes kernel state into the FlatBuffers update frame
//! that drives every UI update.
//!
//! `Kernel::make_update` is the hot path called at up to 4 Hz. It:
//! 1. Calls `visible_items()` to compute the current timeline item list.
//! 2. Diffs against `last_emitted_items` to compute `inserted`/`updated`/`removed`.
//! 3. Assembles `KernelSnapshot` with `Metrics` counters and all projections.
//! 4. Encodes the snapshot once and hands the binary frame to the caller.
//!
//! Performance invariants (see `make_update_us` / `serialize_us` metrics):
//! - No store scans on the hot path — all aggregates maintained incrementally.
//! - Each `run_snapshot_projections()` call is non-blocking (D8: no polling).
//! - `last_payload_bytes` lags one tick to avoid double-serialization.

use super::{diff_items, ratio, Instant, Kernel, KernelSnapshot, Metrics, DEFAULT_EMIT_HZ};
use crate::update_envelope::{encode_snapshot_with_typed, UpdateFrameBytes};

mod helpers;
mod projections;
mod views;

/// Snapshot schema version stamped into every emitted `KernelUpdate`.
///
/// This is a re-export of the canonical [`crate::update_envelope::SNAPSHOT_SCHEMA_VERSION`]
/// so the snapshot emitter and the wire-envelope contract can never drift to
/// two different numbers. Bump it at the canonical site on any breaking field
/// rename, removal, or type change.
///
/// If `schema_version` doesn't match the version the host was compiled
/// against, the host should show an error and refuse to decode further —
/// **do not silently ignore unknown fields**. A renamed or retyped field
/// otherwise decodes to wrong/null data with no diagnostic signal; shells on
/// a mismatched version log and degrade (D1) rather than mis-decode.
pub const KERNEL_SCHEMA_VERSION: u32 = crate::update_envelope::SNAPSHOT_SCHEMA_VERSION;

impl Kernel {
    pub(crate) fn make_update(&mut self, running: bool) -> UpdateFrameBytes {
        let emit_started = Instant::now();
        // Wall-clock stamp for the actor-thread liveness heartbeat. `Instant`
        // above is monotonic and cannot be compared to a shell-side clock, so
        // a separate wall-clock reading is required. D7 / D9: the kernel owns
        // time — route through the injected `Clock` via `now_ms()` so
        // deterministic replay and tests observe the same `last_tick_ms` the
        // production tick emitted. `now_ms()` already collapses a pre-epoch
        // clock to `0` (D6: no panic at the public boundary).
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

        let items = self.visible_items();
        let (inserted, updated, removed) = diff_items(&self.last_emitted_items, &items);
        self.last_emitted_items = items.clone();

        // "Profiled" = has a kind:0 picture URL (None signals no kind:0
        // or no `picture` field in the parsed metadata — aim.md §2,
        // backend ships raw Option, presentation picks the fallback).
        let visible_profiled_items = items
            .iter()
            .filter(|item| item.author_picture_url.is_some())
            .count();
        let visible_placeholder_avatar_items = items.len().saturating_sub(visible_profiled_items);
        let counters = self.total_counters();
        let update = KernelSnapshot {
            rev: self.rev,
            schema_version: KERNEL_SCHEMA_VERSION,
            last_tick_ms,
            update_kind: "ViewBatch",
            running,
            // D0: the views cluster (`profile`, the visible timeline,
            // `author_view`, `thread_view`, and the `inserted` / `updated` /
            // `removed` deltas) is no longer a typed field set — all seven are
            // inserted into `projections` below under their built-in keys by
            // `snapshot_projections_with_publish_cluster`. The `items`,
            // `inserted`, `updated`, and `removed` locals stay live: they still
            // feed the `metrics` counters and the `NMP_PERF` log line.
            metrics: Metrics {
                generated_events: counters.events_rx,
                // Diagnostic counters maintained incrementally at the `events`
                // ingest/mutation sites — no per-emit HashMap scan (the 60 Hz
                // snapshot path must stay O(1) in cached-event count).
                note_events: self.metric_note_events,
                profile_events: self.profiles.len() as u64,
                duplicate_events: self.metric_duplicate_events,
                delete_events: 0,
                // `metric_stored_events` tracks `events.len()` (an O(1) read on
                // its own); the profiles + seed_contacts terms are O(1) `len()`
                // calls, so the historical sum is preserved unchanged.
                stored_events: self.metric_stored_events as usize
                    + self.profiles.len()
                    + self.seed_contacts.len(),
                tombstones: 0,
                visible_items: self.last_emitted_items.len(),
                visible_profiled_items,
                visible_placeholder_avatar_items,
                open_views: self.logical_interests().len() as u32,
                events_since_last_update: self.events_since_last_update,
                diagnostic_firehose_events: self.diagnostic_firehose.events,
                inserted_count: inserted.len(),
                updated_count: updated.len(),
                removed_count: removed.len(),
                events_per_second_configured: 0,
                emit_hz_configured: DEFAULT_EMIT_HZ,
                update_sequence: self.update_sequence,
                estimated_store_bytes: self.estimated_store_bytes(),
                // Diagnostic only. Sourced from the PREVIOUS tick's serialized
                // length so this struct is serialized exactly once below
                // (no serialize-then-discard just to size the field). `0` on
                // the very first tick; lags the real snapshot by one tick.
                payload_bytes: self.last_payload_bytes,
                store_to_payload_ratio: ratio(
                    self.estimated_store_bytes(),
                    self.last_payload_bytes,
                ),
                // G-S4 — live actor command-channel depth from the straddle
                // counter (`NmpApp::send_cmd` increments, the actor loop
                // decrements). Zero when the kernel runs outside the actor
                // (tests, codegen) — no handle bound. Saturates at `u32::MAX`.
                actor_queue_depth: self.actor_queue_depth(),
                frames_rx: counters.frames_rx,
                events_rx: counters.events_rx,
                eose_rx: counters.eose_rx,
                notices_rx: counters.notices_rx,
                closed_rx: counters.closed_rx,
                bytes_rx: counters.bytes_rx,
                bytes_tx: counters.bytes_tx,
                contacts_authors: self.seed_contacts.values().map(Vec::len).sum(),
                timeline_authors: self.timeline_authors.len(),
                first_event_ms: self.elapsed_ms(self.timing.first_event_at),
                target_profile_loaded_ms: self.elapsed_ms(self.timing.target_profile_loaded_at),
                timeline_opened_ms: self.elapsed_ms(self.timing.timeline_opened_at),
                timeline_first_item_ms: self.elapsed_ms(self.timing.timeline_first_item_at),
                update_emitted_ms: self.elapsed_ms(Some(emit_started)),
                last_event_to_emit_ms,
                max_event_to_emit_ms: self.max_event_to_emit_ms,
                max_events_per_update: self.max_events_per_update,
                // T114b — per-dispatch retention audit visibility.
                dispatch_drops_total: self.dispatch_drops_total(),
                claim_drops_total: self.claim_drops_total(),
                make_update_us: self.last_make_update_us,
                serialize_us: self.last_serialize_us,
                update_frame_degradations_total: self.update_frame_degradations_total,
            },
            relay_status: self.relay_status(),
            relay_statuses: self.relay_statuses(),
            logical_interests: self.logical_interests(),
            wire_subscriptions: self.wire_subscriptions(),
            logs: self.logs.iter().cloned().collect(),
            // D0: identity output (`accounts`, `active_account`) is no longer a
            // typed field — both are inserted into `projections` below under the
            // built-in keys `"accounts"` / `"active_account"` by
            // `snapshot_projections_with_publish_cluster`.
            last_error_toast: self.last_error_toast_snapshot().cloned(),
            last_error_category: self.last_error_category_snapshot().cloned(),
            // #171 (D6): project the recorded planner error so the host can
            // observe a genuine structural compile failure instead of silent
            // empty frames. `None` (→ JSON null) in steady state.
            last_planner_error: self.lifecycle.last_planner_error().map(str::to_owned),
            // D0: NIP-47 NWC wallet state and NIP-46 bunker handshake state are
            // no longer kernel fields — both are app nouns surfaced via
            // host-registered snapshot projections (`"wallet"` /
            // `"bunker_handshake"`) collected in `projections` below.
            //
            // D0: the publish / relay-settings cluster (`publish_queue`,
            // `publish_outbox`, `relay_edit_rows`, `relay_role_options`) is
            // likewise app-shaped relay/publish state and is no longer a typed
            // field set — `snapshot_projections_with_publish_cluster` inserts
            // them into the same `projections` map under built-in keys.
            //
            // Host-extensible snapshot output: run every host-registered
            // projection closure and append its namespaced JSON value, then
            // add the kernel-owned publish cluster. Empty (and
            // `skip_serializing_if`'d off the wire) only when no host
            // registered a projection AND the publish cluster contributes no
            // keys — in practice the publish keys are always present, matching
            // the old typed fields' always-emitted shape.
            // D8: the host closures run on this actor thread inside the tick;
            // `run_snapshot_projections` documents the non-blocking contract.
            //
            // D0: the views cluster (`profile`, `timeline`, `author_view`,
            // `thread_view`, `inserted`, `updated`, `removed`) is folded into
            // the same map. `items` / `inserted` / `updated` / `removed` are
            // tick-local bindings, so they are passed in; `profile_card()`,
            // `author_view()`, and `thread_view()` read `&self` and are called
            // inside the helper.
            projections: self
                .snapshot_projections_with_publish_cluster(&items, &inserted, &updated, &removed),
        };

        // Encode the snapshot exactly once. The on-wire `payload_bytes`
        // metric above already reflects the previous tick's size; the perf log
        // below uses this tick's true length so the diagnostic stays accurate.
        // Capture the encode start so we can report "build" vs "encode" time.
        let before_serialize = Instant::now();
        let snapshot = match serde_json::to_value(&update) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.update_frame_degradations_total =
                    self.update_frame_degradations_total.saturating_add(1);
                self.log(format!(
                    "NMP_DEGRADATION update_snapshot_to_value_failed rev={} error={error}",
                    self.rev
                ));
                serde_json::json!({
                    "schema_version": KERNEL_SCHEMA_VERSION,
                    "rev": self.rev,
                    "last_tick_ms": last_tick_ms,
                    "update_kind": "ViewBatch",
                    "running": running,
                    "metrics": {
                        "update_frame_degradations_total": self.update_frame_degradations_total
                    },
                    "last_error_category": "transport",
                    "last_error_toast": "Update transport degraded"
                })
            }
        };
        // ADR-0037: run every host-registered typed projection and carry its
        // opaque FlatBuffers bytes in the frame's `typed_projections` sidecar,
        // alongside the generic `Value` snapshot. During migration both
        // representations are emitted for a piloted key (e.g. `"nmp.feed.home"`):
        // an un-migrated host keeps reading the generic subtree, a migrated host
        // prefers the typed sidecar. `nmp-core` never interprets the bytes — the
        // closures live in app/protocol crates. Empty when nothing is
        // registered, in which case `encode_snapshot_with_typed` produces wire
        // bytes byte-identical to the legacy `encode_snapshot_value`.
        // D8: these closures run on this actor thread inside the tick;
        // `run_typed_projections` documents the non-blocking contract.
        let typed = self.run_typed_projections();
        let encoded = encode_snapshot_with_typed(snapshot, &typed);
        // Compute this tick's timing immediately after encode; the log below
        // uses these current values while the snapshot above carries the previous
        // tick's values (one-tick lag, same pattern as `payload_bytes`).
        let this_serialize_us = before_serialize.elapsed().as_micros();
        let this_make_update_us = emit_started.elapsed().as_micros();
        if batch_events > 0 || !inserted.is_empty() || !updated.is_empty() || !removed.is_empty() {
            self.log(format!(
                "NMP_PERF rust_update rev={} batch_events={} inserted={} updated={} removed={} visible={} payload_bytes={} make_update_us={} serialize_us={} event_to_emit_ms={} max_event_to_emit_ms={}",
                self.rev,
                batch_events,
                inserted.len(),
                updated.len(),
                removed.len(),
                self.last_emitted_items.len(),
                encoded.len(),
                this_make_update_us,
                this_serialize_us,
                last_event_to_emit_ms
                    .map_or_else(|| "none".to_string(), |value| value.to_string()),
                self.max_event_to_emit_ms
            ));
        }
        self.events_since_last_update = 0;
        self.changed_since_emit = false;
        // One-tick-lag diagnostics: store this tick's measurements so the
        // NEXT tick's Metrics reflect them. Same pattern as `last_payload_bytes`.
        self.last_serialize_us = this_serialize_us;
        self.last_make_update_us = this_make_update_us;
        self.last_payload_bytes = encoded.len();
        encoded
    }

    #[cfg(test)]
    pub(crate) fn make_update_value_for_test(&mut self, running: bool) -> serde_json::Value {
        crate::update_envelope::decode_snapshot_payload(&self.make_update(running))
            .unwrap_or(serde_json::Value::Null)
    }

    #[cfg(test)]
    pub(crate) fn make_update_json_for_test(&mut self, running: bool) -> String {
        serde_json::to_string(&self.make_update_value_for_test(running)).unwrap_or_default()
    }
}
