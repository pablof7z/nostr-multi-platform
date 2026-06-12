//! Snapshot emission: encodes kernel state into the FlatBuffers update frame
//! that drives every UI update.
//!
//! `Kernel::make_update` is the hot path called at up to 4 Hz. It:
//! 1. Assembles `KernelSnapshot` with `Metrics` counters and all projections.
//! 2. Encodes the snapshot once and hands the binary frame to the caller.
//!
//! Step 3A (issue #920): the follow-feed projection cluster (`timeline` /
//! `inserted` / `updated` / `removed`) has been removed from the kernel — the
//! kernel no longer computes a per-tick `visible_items()` list or diffs it. The
//! six feed-derived `Metrics` fields are retained at `0` because `KernelMetrics`
//! is a frozen codegen type; they clear fully when the type's shape migrates.
//!
//! Performance invariants (see `make_update_us` / `serialize_us` metrics):
//! - No store scans on the hot path — all aggregates maintained incrementally.
//! - Each `run_snapshot_projections()` call is non-blocking (D8: no polling).
//! - `last_payload_bytes` lags one tick to avoid double-serialization.

use super::{ratio, Instant, Kernel, KernelSnapshot, Metrics, DEFAULT_EMIT_HZ};
use crate::update_envelope::{encode_snapshot_with_envelope, UpdateFrameBytes};

mod helpers;
mod projections;
mod views;

pub use projections::KERNEL_BUILTIN_PROJECTION_KEYS;

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
    /// Build the `KernelSnapshot` struct for the current tick. Called by both
    /// `make_update` (production) and the `#[cfg(test)]` helpers so the two
    /// paths never drift. Does NOT mutate `rev` / `update_sequence` / timing
    /// accumulators — those are updated by `make_update` before calling here.
    ///
    /// `emit_started` is the `Instant` captured at the start of the tick;
    /// it is passed in rather than re-captured so the production and test
    /// paths both see a consistent value without a second `Instant::now()`.
    #[allow(clippy::too_many_lines)] // struct literal — intentionally dense
    fn build_snapshot_struct(
        &mut self,
        running: bool,
        last_tick_ms: u64,
        emit_started: Instant,
        last_event_to_emit_ms: Option<u128>,
    ) -> KernelSnapshot {
        let counters = self.total_counters();
        KernelSnapshot {
            rev: self.rev,
            schema_version: KERNEL_SCHEMA_VERSION,
            last_tick_ms,
            update_kind: "ViewBatch",
            running,
            // D0: the views cluster (`profile`, `author_view`, `thread_view`) is
            // no longer a typed field set — they are inserted into `projections`
            // below under their built-in keys by
            // `snapshot_projections_with_publish_cluster`.
            //
            // Step 3A (issue #920): the follow-feed cluster (`timeline` /
            // `inserted` / `updated` / `removed`) has been removed. The kernel no
            // longer computes a `visible_items()` list or diffs it, so the six
            // feed-derived metrics below are now constant `0`. They are retained
            // (not deleted) because `KernelMetrics` is a frozen codegen type —
            // changing its shape would be a Swift-affecting wire break. They
            // clear fully when the type's shape migrates in a later step.
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
                // Step 3A (#920): feed cluster removed — constant `0` until the
                // frozen `KernelMetrics` shape migrates.
                visible_items: 0,
                visible_profiled_items: 0,
                visible_placeholder_avatar_items: 0,
                open_views: self.logical_interests().len() as u32,
                events_since_last_update: self.events_since_last_update,
                diagnostic_firehose_events: self.diagnostic_firehose.events,
                // Step 3A (#920): feed delta cluster removed — constant `0`.
                inserted_count: 0,
                updated_count: 0,
                removed_count: 0,
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
            // V-67 (D6): surface the LMDB open failure so the host observes the
            // degraded-store state on every tick instead of silently losing all
            // persisted events. Omitted from the wire when `None`
            // (`skip_serializing_if`) to keep the snapshot size unchanged for
            // healthy (no-failure) sessions.
            store_open_failure: self.store_open_failure.clone(),
            // V-66 (D3): when an account is active but configured_relays is empty
            // every outbound socket connects to the hardcoded FALLBACK relays.
            // The fallback keeps the app functional, but must no longer be
            // silent — the host needs to know it is running on unconfigured
            // defaults so it can surface a banner / alert to the user.
            // `Some(true)` iff signed-in + no rows; absent from the wire
            // (`skip_serializing_if`) in all other states so healthy sessions
            // produce byte-identical snapshots to pre-V-66 builds.
            no_configured_relays: if self.active_account.is_some()
                && self.configured_relays.is_empty()
            {
                Some(true)
            } else {
                None
            },
            // D0: NIP-47 NWC wallet state and NIP-46 bunker handshake state are
            // no longer kernel fields — both are app nouns surfaced via
            // host-registered snapshot projections (`"wallet"` /
            // `"bunker_handshake"`) collected in `projections` below.
            //
            // D0: the publish / relay-settings cluster (`publish_queue`,
            // `publish_outbox`, `configured_relays`, `relay_role_options`) is
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
            // D0: the views cluster (`profile`, `author_view`, `thread_view`) is
            // folded into the same map. `profile_card()`, `author_view()`, and
            // `thread_view()` read `&self` and are called inside the helper.
            // Step 3A (#920): the follow-feed cluster (`timeline` / `inserted` /
            // `updated` / `removed`) is no longer produced here.
            projections: self.snapshot_projections_with_publish_cluster(),
        }
    }

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

        let update = self.build_snapshot_struct(running, last_tick_ms, emit_started, last_event_to_emit_ms);

        // Capture the encode start so we can report "build" vs "encode" time.
        let before_serialize = Instant::now();
        // ADR-0037: run every host-registered typed projection and carry its
        // opaque FlatBuffers bytes in the frame's `typed_projections` sidecar.
        // D8: these closures run on this actor thread inside the tick;
        // `run_typed_projections` documents the non-blocking contract.
        let typed = self.run_typed_projections();
        // Fire every host-registered per-tick observer (the generic, data-free
        // counterpart to the projection registry — see
        // `SnapshotRegistry::run_tick_observers`). These contribute no snapshot
        // output; they are pure per-tick side-effect reconcilers (e.g. the
        // NIP-57 zap-subscription reconciler that diffs the active pubkey and
        // enqueues Push/Withdraw interest each tick). D8: they run on this actor
        // thread inside the tick and MUST be non-blocking (enqueue only); D6:
        // each is wrapped in `catch_unwind` so a panicking observer can never
        // unwind the actor thread into a terminal `Panic` frame.
        self.run_tick_observers();
        // Wave C (ADR-0037): merge the kernel-owned (Tier-2) built-in typed
        // sidecars with the host-registered (Tier-1) ones. These read live
        // `&self` state, so — unlike a `register_typed` closure — they are
        // emitted directly here. See `kernel::typed_projections` for the
        // mechanism rationale and the per-built-in template.
        //
        // Built-in keys win on collision, mirroring the documented JSON rule in
        // `snapshot_projections_with_publish_cluster` ("Built-in keys win on
        // collision … so the kernel-owned value stays authoritative"). The
        // host-side consumer matches by first key (`projections.first(where:)`),
        // so a colliding host entry must be dropped — not merely appended — or it
        // would shadow the built-in and silently contradict the JSON contract.
        let typed = self.merge_builtin_typed_projections(typed);
        // ADR-0044 / PR-B (#991/#979): emit only the typed Tier-3 envelope +
        // typed-projection sidecar. The generic `payload:Value` slot is
        // intentionally absent from the wire (set to `None` in
        // `encode_snapshot_with_envelope`). No JSON serialization of the
        // `KernelSnapshot` struct occurs on the production path — the struct
        // is encoded directly into the Tier-3 FlatBuffers fields. For Rust
        // test helpers that still need a JSON view, use `make_update_value_for_test`
        // which serializes the struct directly (no wire roundtrip needed).
        let encoded = encode_snapshot_with_envelope(&typed, &update);
        // Compute this tick's timing immediately after encode; the log below
        // uses these current values while the snapshot above carries the previous
        // tick's values (one-tick lag, same pattern as `payload_bytes`).
        let this_serialize_us = before_serialize.elapsed().as_micros();
        let this_make_update_us = emit_started.elapsed().as_micros();
        // Step 3A (#920): the feed delta cluster (`inserted` / `updated` /
        // `removed` / `visible`) was removed, so the perf line is gated on
        // `batch_events` alone and no longer reports the feed counters.
        if batch_events > 0 {
            self.log(format!(
                "NMP_PERF rust_update rev={} batch_events={} payload_bytes={} make_update_us={} serialize_us={} event_to_emit_ms={} max_event_to_emit_ms={}",
                self.rev,
                batch_events,
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

    /// PR-B (#991/#979): drive `make_update` for one tick and return BOTH the
    /// raw `UpdateFrameBytes` AND a `serde_json::Value` serialized from the same
    /// tick's `KernelSnapshot` struct (without a wire roundtrip).
    ///
    /// Used by `tier3_envelope_tests` to compare typed Tier-3 FlatBuffers fields
    /// against the struct-serialized JSON on the SAME tick — the two were
    /// previously both decoded from the same frame bytes (before payload zeroing).
    /// Now JSON comes from the struct and the typed fields come from the wire.
    /// Rev and all other fields still agree because both come from the same tick.
    #[cfg(test)]
    pub(crate) fn make_update_frame_and_json_for_test(
        &mut self,
        running: bool,
    ) -> (crate::update_envelope::UpdateFrameBytes, serde_json::Value) {
        let emit_started = Instant::now();
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
        let update = self.build_snapshot_struct(running, last_tick_ms, emit_started, last_event_to_emit_ms);
        let json = serde_json::to_value(&update).unwrap_or(serde_json::Value::Null);
        let before_serialize = Instant::now();
        let typed = self.run_typed_projections();
        self.run_tick_observers();
        let typed = self.merge_builtin_typed_projections(typed);
        let frame = encode_snapshot_with_envelope(&typed, &update);
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
    /// `run_tick_observers`, `run_typed_projections`, and
    /// `merge_builtin_typed_projections`), then serialize the `KernelSnapshot`
    /// struct to `serde_json::Value` for test assertions.
    ///
    /// The `payload:Value` slot is no longer emitted on the wire (the decoder
    /// itself is deleted), so JSON-shaped assertions cannot come off the frame.
    /// Test helpers serialize the struct directly — equivalent coverage,
    /// no wire roundtrip.
    ///
    /// `run_tick_observers()` is called, matching production semantics (tests
    /// that count observer invocations get exact per-tick counts).
    #[cfg(test)]
    pub(crate) fn make_update_value_for_test(&mut self, running: bool) -> serde_json::Value {
        let emit_started = Instant::now();
        let last_tick_ms = self.now_ms();
        self.rev = self.rev.saturating_add(1);
        self.update_sequence = self.update_sequence.saturating_add(1);
        let last_event_to_emit_ms = self
            .timing
            .last_event_at
            .map(|last_event_at| emit_started.duration_since(last_event_at).as_millis());
        let snapshot = self.build_snapshot_struct(running, last_tick_ms, emit_started, last_event_to_emit_ms);
        // Run the same side-effect hooks that `make_update` runs, so tests
        // observing tick observers / typed projection closures see the same
        // per-tick semantics.
        let _typed_host = self.run_typed_projections();
        self.run_tick_observers();
        let _typed_merged = self.merge_builtin_typed_projections(_typed_host);
        serde_json::to_value(&snapshot).unwrap_or(serde_json::Value::Null)
    }

    #[cfg(test)]
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
    #[cfg(test)]
    pub(crate) fn make_update_typed_for_test(
        &mut self,
        running: bool,
    ) -> (
        serde_json::Value,
        Vec<crate::update_envelope::TypedProjectionData>,
    ) {
        let (frame, value) = self.make_update_frame_and_json_for_test(running);
        let typed = crate::update_envelope::decode_snapshot_typed_projections(&frame)
            .unwrap_or_default();
        (value, typed)
    }
}
