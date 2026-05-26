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

use super::{
    diff_items, event_references, first_event_ref, ratio, referenced_event_ids, root_event_id,
    truncate, AccountSummary, AuthorViewPayload, BTreeSet, ClaimedEventDto, Instant, Kernel,
    KernelSnapshot, MentionProfilePayload, Metrics, Profile, ProfileAction, ProfileCard,
    ProfileDispatchSpec, SettingsHubSummary, StoredEvent, ThreadViewPayload, TimelineItem,
    DEFAULT_EMIT_HZ,
};
use crate::update_envelope::{encode_snapshot_value, UpdateFrameBytes};

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
        let encoded = encode_snapshot_value(snapshot);
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

    /// Collect the snapshot `projections` map: every host-registered
    /// projection closure plus the kernel-owned built-in projections (the
    /// publish / relay-settings cluster, the identity pair, and the views cluster).
    ///
    /// D0: `publish_queue`, `publish_outbox`, `relay_edit_rows`, and
    /// `relay_role_options` are app-shaped relay/publish state; `accounts` /
    /// `active_account` are identity output; and the views cluster (`profile`,
    /// `timeline`, `author_view`, `thread_view`, `inserted`, `updated`,
    /// `removed`) is app-shaped social view state — none are protocol-neutral
    /// kernel primitives, so none carry a typed `KernelSnapshot` field. Unlike
    /// the host-registered `"wallet"` / `"bunker_handshake"` projections (which
    /// read actor-runtime slots through a no-arg closure), these are
    /// kernel-owned, so they cannot be expressed as a `SnapshotRegistry`
    /// closure — they are inserted here directly after the host closures run.
    ///
    /// The views-cluster deltas (`items`, `inserted`, `updated`, `removed`)
    /// are tick-local values computed in `make_update`, so they are passed in
    /// by reference; `profile_card()`, `author_view()`, and `thread_view()`
    /// read `&self` and are called inside this helper. The generic typed-field
    /// name `items` is deliberately surfaced under the more descriptive
    /// projection key `"timeline"`.
    ///
    /// Built-in keys win on collision: a host that registers `"publish_queue"`,
    /// `"publish_outbox"`, `"relay_edit_rows"`, `"relay_role_options"`,
    /// `"settings_hub"`, `"accounts"`, `"active_account"`, `"profile"`,
    /// `"timeline"`, `"author_view"`, `"thread_view"`, `"inserted"`,
    /// `"updated"`, or `"removed"` is overwritten so the kernel-owned value
    /// stays authoritative. A serialization failure degrades to a stable empty
    /// value (`[]` for the lists, `null` for the optional
    /// payloads) — D6: never a panic at the snapshot boundary — and the key is
    /// still present, mirroring the old always-emitted typed fields.
    fn snapshot_projections_with_publish_cluster(
        &mut self,
        items: &[TimelineItem],
        inserted: &[TimelineItem],
        updated: &[TimelineItem],
        removed: &[String],
    ) -> std::collections::HashMap<String, serde_json::Value> {
        let mut projections = self.run_snapshot_projections();
        projections.insert(
            "publish_queue".to_string(),
            serde_json::to_value(self.publish_queue_snapshot()).unwrap_or(serde_json::Value::Null),
        );
        projections.insert(
            "publish_outbox".to_string(),
            serde_json::to_value(self.publish_outbox_items()).unwrap_or(serde_json::Value::Null),
        );
        // D0: outbox header summary — `OutboxSummarySnapshot`. The kernel owns
        // the per-status counters AND the English `title` / `subtitle`
        // strings (§6 anti-pattern #1); shells bind the strings verbatim
        // instead of `.filter`-counting `publish_outbox` to derive them.
        projections.insert(
            "outbox_summary".to_string(),
            serde_json::to_value(self.outbox_summary_snapshot()).unwrap_or(serde_json::Value::Null),
        );
        projections.insert(
            "relay_edit_rows".to_string(),
            serde_json::to_value(self.relay_edit_rows_snapshot())
                .unwrap_or(serde_json::Value::Null),
        );
        projections.insert(
            "relay_role_options".to_string(),
            serde_json::to_value(crate::actor::relay_role_options())
                .unwrap_or(serde_json::Value::Null),
        );
        // Settings-hub view projection. Currently a single pre-formatted
        // relays subtitle ("N relays" / "1 relay" / "No relays configured")
        // — aim.md §6/AP1 forbids the iOS shell from owning that
        // pluralization. Built locally next to `relay_edit_rows` so the two
        // can never drift out of sync. A serialization failure degrades to
        // `null` so the key is omitted, mirroring the publish-cluster pattern.
        let settings_hub =
            SettingsHubSummary::from_relay_edit_rows(self.relay_edit_rows_snapshot());
        projections.insert(
            "settings_hub".to_string(),
            serde_json::to_value(&settings_hub).unwrap_or(serde_json::Value::Null),
        );
        // Direction review #29: drain EVERY terminal that settled since the
        // last emit into the `action_results` array. The host can clear a
        // per-action spinner (published / failed / cancelled) without polling.
        // If two actions settled in the same tick the host sees both, so no
        // spinner hangs. This key is absent in steady state (drain returns
        // `Null` → not inserted) and a `[{correlation_id, status, error}, ...]`
        // array whenever any action settled this tick. The host resolves each
        // spinner by correlation_id.
        let action_results = self.take_action_results_projection();
        if !action_results.is_null() {
            projections.insert("action_results".to_string(), action_results);
        }
        // Snapshot mirror of every in-flight action's lifecycle stages,
        // keyed by `correlation_id`. Unlike `action_results` (drain on emit),
        // `action_stages` is a *copy* — the same correlation_id reappears on
        // every tick until the host calls `nmp_app_ack_action_stage`. The host
        // renders a progress indicator from the latest stage in each id's
        // history and clears it on the terminal stage (`Accepted` / `Failed`)
        // before acking. Absent in steady state (`Null` → not inserted).
        let action_stages = self.action_stages_projection();
        if !action_stages.is_null() {
            projections.insert("action_stages".to_string(), action_stages);
        }
        // V5 thin-shell display projection. `action_lifecycle` collapses the
        // per-stage history `action_stages` carries into the host's
        // `{in_flight, recent_terminal}` shape, with TTL-based eviction of
        // terminals (no host ack required). Absent in steady state — same
        // `Null → omit key` convention as `action_results` / `action_stages`.
        // The mutable borrow runs the tracker's TTL sweep on every emit so a
        // quiet kernel still prunes expired terminals.
        let action_lifecycle = self.action_lifecycle_projection();
        if !action_lifecycle.is_null() {
            projections.insert("action_lifecycle".to_string(), action_lifecycle);
        }
        // D0: identity output. `accounts_enriched()` returns `AccountSummary`
        // records patched with kind:0 picture_url / display_name so the toolbar
        // avatar and accounts list show real profile data. `active_account` is
        // still sourced from the raw snapshot (it is just a pubkey string).
        let (_, active_account) = self.account_snapshot();
        let enriched = self.accounts_enriched();
        projections.insert(
            "accounts".to_string(),
            serde_json::to_value(&enriched)
                .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
        );
        projections.insert(
            "active_account".to_string(),
            serde_json::to_value(active_account).unwrap_or(serde_json::Value::Null),
        );
        // D0: views cluster. `profile` is the active-account profile card;
        // `timeline` is the visible item list (renamed from the generic
        // typed-field name `items`); `author_view` / `thread_view` are the
        // open-view payloads (`null` when no view is open); `inserted` /
        // `updated` / `removed` are the per-tick timeline deltas. A
        // serialization failure degrades to `[]` for the lists and `null` for
        // the optional payloads so every key is always present, matching the
        // old always-emitted typed fields.
        projections.insert(
            "profile".to_string(),
            serde_json::to_value(self.profile_card()).unwrap_or(serde_json::Value::Null),
        );
        projections.insert(
            "timeline".to_string(),
            serde_json::to_value(items).unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
        );
        projections.insert(
            "author_view".to_string(),
            serde_json::to_value(self.author_view()).unwrap_or(serde_json::Value::Null),
        );
        projections.insert(
            "thread_view".to_string(),
            serde_json::to_value(self.thread_view()).unwrap_or(serde_json::Value::Null),
        );
        projections.insert(
            "inserted".to_string(),
            serde_json::to_value(inserted).unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
        );
        projections.insert(
            "updated".to_string(),
            serde_json::to_value(updated).unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
        );
        projections.insert(
            "removed".to_string(),
            serde_json::to_value(removed).unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
        );
        // Diagnostics-screen projection. Pre-rolls the relay + wire-sub
        // arrays into one struct with every aggregate (active / EOSE'd /
        // total sub counts, total events_rx) and every display string
        // (relative-time labels, connection / auth / role labels) already
        // computed. Replaces the §4.5 "no derived state" + §6 anti-
        // pattern #1 + §"Where do views live?" violations the three iOS
        // diagnostics views used to commit. See the
        // `kernel/relay_diagnostics.rs` module doc for the exact bible
        // references. Serialization failure degrades to JSON null so the
        // key still appears (mirrors the publish cluster's contract).
        projections.insert(
            "relay_diagnostics".to_string(),
            serde_json::to_value(self.relay_diagnostics_snapshot())
                .unwrap_or(serde_json::Value::Null),
        );
        // `mention_profiles` — derived view (aim.md §4.2): pubkey ->
        // {display, picture_url, avatar_initials, avatar_color} for every
        // author surfaced in ANY currently-open view. Built from the union of
        // the home `timeline` (the `items` parameter, already
        // `visible_items()`), the open `author_view` items, and the open
        // `thread_view` items so HomeFeedView / ThreadScreen / ProfileView
        // all find their authors pre-mapped without reconstructing the dict
        // in Swift (V-31 thin-shell; replaces the Swift Dictionary
        // derivations at `HomeFeedView.swift:187-197` and
        // `ThreadScreen.swift:23-35`). First writer wins on collision —
        // matches `mention_profiles_from_items` semantics. Empty `{}` only
        // when no events are visible and no view is open; never absent (D1).
        let mut mention_profiles = self.mention_profiles_from_items(items);
        for (k, v) in self
            .author_view()
            .map(|av| self.mention_profiles_from_items(&av.items))
            .unwrap_or_default()
        {
            mention_profiles.entry(k).or_insert(v);
        }
        for (k, v) in self
            .thread_view()
            .map(|tv| self.mention_profiles_from_items(&tv.items))
            .unwrap_or_default()
        {
            mention_profiles.entry(k).or_insert(v);
        }
        projections.insert(
            "mention_profiles".to_string(),
            serde_json::to_value(&mention_profiles)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default())),
        );
        // `claimed_events` projection — keyed by `primary_id` (hex64 event
        // id for nevent/note URIs; `kind:pubkey:d_tag` coordinate for
        // naddr URIs). Built by walking the current `event_claims` set
        // and looking each key up against `self.events` via
        // `lookup_for_primary_id`. Missing entries are silently absent —
        // the host renders the URI as-is until the event arrives (D1
        // best-effort; D8 push semantics on the next snapshot tick).
        //
        // BTreeMap for deterministic key ordering (snapshot diff
        // stability across ticks); serialisation degrades to `{}` on
        // failure, mirroring `mention_profiles`.
        let mut claimed_events: std::collections::BTreeMap<String, ClaimedEventDto> =
            std::collections::BTreeMap::new();
        for key in self.event_claims.keys() {
            if let Some(stored) = self.lookup_for_primary_id(key) {
                claimed_events.insert(
                    key.clone(),
                    ClaimedEventDto::from_stored(key.clone(), stored),
                );
            }
        }
        projections.insert(
            "claimed_events".to_string(),
            serde_json::to_value(&claimed_events)
                .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default())),
        );
        projections
    }

    /// Look up the `StoredEvent` that resolves a `claim_event`
    /// `primary_id`. Hex-64 keys (event id form) index `self.events`
    /// directly; coordinate keys (`kind:pubkey:d_tag`) scan
    /// `self.events.values()` for the matching addressable triple.
    ///
    /// d-tags may legally contain `:` (rare but spec-allowed); the
    /// split is bounded to the first two colons so a d-tag like
    /// `"foo:bar"` round-trips correctly.
    pub(super) fn lookup_for_primary_id(&self, key: &str) -> Option<&StoredEvent> {
        if is_hex64_lower(key) {
            return self.events.get(key);
        }
        let mut parts = key.splitn(3, ':');
        let kind = parts.next().and_then(|s| s.parse::<u32>().ok())?;
        let pubkey = parts.next()?;
        let d_tag = parts.next()?;
        self.events.values().find(|e| {
            e.kind == kind
                && e.author == pubkey
                && e.tags
                    .iter()
                    .any(|t| t.len() >= 2 && t[0] == "d" && t[1] == d_tag)
        })
    }

    pub(super) fn visible_items(&self) -> Vec<TimelineItem> {
        self.timeline
            .iter()
            .filter_map(|id| self.events.get(id))
            .take(self.visible_limit)
            .map(|event| self.timeline_item(event))
            .collect()
    }

    pub(super) fn timeline_item(&self, event: &StoredEvent) -> TimelineItem {
        let profile = self.profile_for_pubkey(&event.author);
        // aim.md §2: picture URL stays `Option<String>`. No identicon
        // placeholder is substituted in NMP; presentation layers choose
        // the missing-picture strategy.
        let author_picture_url = profile
            .and_then(|p| p.picture_url.as_deref())
            .filter(|url| !url.is_empty())
            .map(str::to_owned);
        // NIP-18 kind:6: the repost's `content` field carries the
        // verbatim stringified inner event JSON. We resolve it once here
        // so the shell binds `nav_target_id` / `repost_inner_content`
        // verbatim and never touches the JSON.
        //
        // D1 best-effort: when `content` is empty or malformed JSON,
        // the shell-visible fallbacks (`event.id`, `""`) match prior
        // behaviour — the "Repost" badge alone communicates state.
        let is_repost = event.kind == 6;
        let (nav_target_id, repost_inner_content) = if is_repost {
            let (inner_id, inner_content) = parse_repost_inner(&event.content);
            (
                inner_id.unwrap_or_else(|| event.id.clone()),
                inner_content.unwrap_or_default(),
            )
        } else {
            (event.id.clone(), String::new())
        };
        TimelineItem {
            id: event.id.clone(),
            author_pubkey: event.author.clone(),
            author_picture_url,
            // NIP-57 — pre-extracted lightning address / LNURL from the
            // author's kind:0 (or `None` when no kind:0 has arrived or
            // it carried no lud16/lud06). Surfaced here so the shell zap
            // button toggles enabled/disabled without a separate profile
            // lookup. Rust decides zapability.
            author_lnurl: profile.and_then(|p| p.lnurl.clone()),
            kind: event.kind,
            content: truncate(&event.content, 1_200),
            content_preview: if is_repost && event.content.trim().is_empty() {
                "Repost".to_string()
            } else {
                truncate(&event.content.replace('\n', " "), 180)
            },
            // aim.md §2 — raw Unix seconds; the presentation layer
            // formats the relative-time label.
            created_at: event.created_at,
            relay_count: event.relay_count,
            is_repost,
            nav_target_id,
            repost_inner_content,
        }
    }

    pub(super) fn profile_card(&self) -> ProfileCard {
        match self.active_account.as_deref() {
            Some(pk) => self.profile_card_for(pk, None, "Waiting for kind:0 from indexer"),
            None => self.profile_card_for("", None, "Waiting for kind:0 from indexer"),
        }
    }

    pub(super) fn profile_card_for(
        &self,
        pubkey: &str,
        npub: Option<&str>,
        placeholder_about: &str,
    ) -> ProfileCard {
        let profile = self.profile_for_pubkey(pubkey);
        // aim.md §2 — picture URL stays `Option<String>` (no identicon
        // placeholder substituted in NMP).
        let picture_url = profile
            .and_then(|p| p.picture_url.as_deref())
            .filter(|url| !url.is_empty())
            .map(str::to_owned);
        let npub_str = npub.unwrap_or(pubkey).to_string();
        let display_name = profile
            .map(|profile| profile.display.clone())
            .filter(|display| !display.is_empty());
        ProfileCard {
            pubkey: pubkey.to_string(),
            npub: npub_str,
            display_name,
            picture_url,
            nip05: profile
                .map(|profile| profile.nip05.clone())
                .unwrap_or_default(),
            about: profile.map_or_else(
                || placeholder_about.to_string(),
                |profile| truncate(&profile.about.replace('\n', " "), 220),
            ),
            has_profile: profile.is_some(),
            // NIP-57 — pre-extracted lightning address / LNURL from
            // kind:0 (lud16 preferred over lud06). `None` when no
            // kind:0 has arrived OR the metadata had no lnurl.
            lnurl: profile.and_then(|p| p.lnurl.clone()),
        }
    }

    fn profile_for_pubkey(&self, pubkey: &str) -> Option<&Profile> {
        match (
            self.profiles.get(pubkey),
            self.local_profile_intents.get(pubkey),
        ) {
            (Some(stored), Some(intent)) if intent.created_at > stored.created_at => Some(intent),
            (Some(stored), _) => Some(stored),
            (None, Some(intent)) => Some(intent),
            (None, None) => None,
        }
    }

    pub(super) fn profile_action_for(&self, pubkey: &str) -> Option<ProfileAction> {
        if pubkey.is_empty() {
            return None;
        }
        let active = self.active_account.as_deref()?;
        if active == pubkey {
            // edit_profile is a LOCAL-UI intent (open the edit sheet) —
            // there is no registered ActionModule for it. `dispatch: None`
            // lets the shell branch on presence-of-dispatch rather than
            // switching on `kind`. (aim.md §4.4: only writes flow through
            // registered ActionModules.)
            return Some(ProfileAction {
                kind: "edit_profile",
                label: "Edit",
                target_pubkey: pubkey.to_string(),
                icon_name: "square.and.pencil",
                dispatch: None,
            });
        }

        let is_following = self
            .seed_contacts
            .get(active)
            .is_some_and(|follows| follows.iter().any(|follow| follow == pubkey));
        let (kind, label, icon_name, namespace) = if is_following {
            ("unfollow", "Unfollow", "person.badge.minus", "nmp.unfollow")
        } else {
            ("follow", "Follow", "person.badge.plus", "nmp.follow")
        };
        // Pre-serialize the action body so the shell sends the same bytes
        // the executor validates: `{"pubkey":"<hex>"}` per
        // `apps/chirp/nmp-app-chirp/src/ffi.rs` (NS_FOLLOW / NS_UNFOLLOW).
        let body_json = serde_json::json!({ "pubkey": pubkey }).to_string();
        Some(ProfileAction {
            kind,
            label,
            target_pubkey: pubkey.to_string(),
            icon_name,
            dispatch: Some(ProfileDispatchSpec {
                namespace,
                body_json,
            }),
        })
    }

    /// Returns the accounts list enriched with profile picture URLs and
    /// real display names from cached kind:0 metadata. The base
    /// `AccountSummary` (built in the identity layer) doesn't see profile
    /// data; we patch here. Per aim.md §2 the patched fields stay
    /// `Option<String>` — when kind:0 carries no display name or no
    /// picture, the field stays `None` and the presentation layer chooses
    /// its own fallback.
    pub(super) fn accounts_enriched(&self) -> Vec<AccountSummary> {
        let (accounts, _) = self.account_snapshot();
        accounts
            .iter()
            .cloned()
            .map(|mut acc| {
                if let Some(profile) = self.profile_for_pubkey(&acc.id) {
                    let real_picture = profile.picture_url.as_deref().filter(|url| !url.is_empty());
                    acc.picture_url = real_picture.map(str::to_owned);
                    if !profile.display.is_empty() {
                        acc.display_name = Some(profile.display.clone());
                    }
                }
                acc
            })
            .collect()
    }

    pub(super) fn author_view(&self) -> Option<AuthorViewPayload> {
        let pubkey = &self.author_view.selected_author.as_ref()?.key;
        let items = self.author_items(pubkey);
        let state = if self.author_view.request_pending {
            "queued"
        } else if items.is_empty() {
            "opening"
        } else {
            "ready"
        };

        let note_count = items.len();
        Some(AuthorViewPayload {
            pubkey: pubkey.clone(),
            state: state.to_string(),
            profile: self.profile_card_for(pubkey, None, "Waiting for selected author kind:0"),
            note_count,
            note_count_display: note_count.to_string(),
            primary_action: self.profile_action_for(pubkey),
            items,
        })
    }

    pub(super) fn author_items(&self, pubkey: &str) -> Vec<TimelineItem> {
        let mut events = self
            .events
            .values()
            .filter(|event| event.author == pubkey && matches!(event.kind, 1 | 6))
            .collect::<Vec<_>>();
        events.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        events
            .into_iter()
            .take(100)
            .map(|event| self.timeline_item(event))
            .collect()
    }

    pub(super) fn thread_view(&self) -> Option<ThreadViewPayload> {
        let focused_id = &self.thread_view.selected_thread.as_ref()?.key;
        let root_id = self
            .thread_root_id(focused_id)
            .unwrap_or_else(|| focused_id.clone());
        let items = self.thread_items(focused_id, &root_id);
        let focused_index = items.iter().position(|item| item.id == *focused_id);
        let previous_count = focused_index.unwrap_or(0);
        let next_count = focused_index.map_or(0, |index| items.len().saturating_sub(index + 1));
        let state = if self.thread_view.request_pending {
            "queued"
        } else if items.is_empty() {
            "opening"
        } else {
            "ready"
        };
        let previous_count_label = format_previous_count_label(previous_count);
        let next_count_label = format_next_count_label(next_count);

        Some(ThreadViewPayload {
            focused_event_id: focused_id.clone(),
            root_event_id: root_id,
            state: state.to_string(),
            items,
            previous_count,
            next_count,
            previous_count_label,
            next_count_label,
        })
    }

    pub(super) fn thread_items(&self, focused_id: &str, root_id: &str) -> Vec<TimelineItem> {
        let mut ids = BTreeSet::new();
        ids.insert(focused_id.to_string());
        ids.insert(root_id.to_string());
        if let Some(focused) = self.events.get(focused_id) {
            ids.extend(referenced_event_ids(focused));
        }

        let mut events = self
            .events
            .values()
            .filter(|event| {
                ids.contains(&event.id)
                    || event_references(event, root_id)
                    || event_references(event, focused_id)
            })
            .collect::<Vec<_>>();
        events.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        events
            .into_iter()
            .take(250)
            .map(|event| self.timeline_item(event))
            .collect()
    }

    pub(super) fn thread_root_id(&self, focused_id: &str) -> Option<String> {
        let event = self.events.get(focused_id)?;
        root_event_id(event)
            .or_else(|| first_event_ref(event))
            .or_else(|| Some(focused_id.to_string()))
    }

    /// Build the `mention_profiles` projection from a slice of timeline
    /// items. Maps `author_pubkey -> MentionProfilePayload` joining
    /// against the kind:0 profile cache. First writer wins on collision
    /// (mirroring the Swift `Dictionary(uniquingKeysWith:)` it replaces).
    /// Per aim.md §2, every payload field that depends on kind:0 is
    /// `Option<String>` — `None` when no kind:0 has arrived for this
    /// author.
    pub(super) fn mention_profiles_from_items(
        &self,
        items: &[TimelineItem],
    ) -> std::collections::HashMap<String, MentionProfilePayload> {
        let mut out: std::collections::HashMap<String, MentionProfilePayload> =
            std::collections::HashMap::new();
        for item in items {
            out.entry(item.author_pubkey.clone()).or_insert_with(|| {
                let profile = self.profile_for_pubkey(&item.author_pubkey);
                let display_name = profile.map(|p| p.display.clone()).filter(|d| !d.is_empty());
                let picture_url = profile
                    .and_then(|p| p.picture_url.as_deref())
                    .filter(|url| !url.is_empty())
                    .map(str::to_owned);
                MentionProfilePayload {
                    pubkey: item.author_pubkey.clone(),
                    display_name,
                    picture_url,
                }
            });
        }
        out
    }
}

/// Extract the two fields a kind:6 row needs from the NIP-18 embedded event
/// JSON: the inner event's `id` (for thread navigation) and `content` (for
/// rendering). Returns `(None, None)` when `raw` is not a JSON object or
/// when neither field is a string, mirroring the Swift `innerEventField`
/// helper that this function replaces.
///
/// Pure, allocation-bounded, no I/O — safe to call on every snapshot tick.
/// This is a display-layer extractor owned by the kernel so the Swift
/// thin-shell does not have to parse Nostr event JSON in the view layer
/// (aim.md §6.9, Chirp thin-shell rule).
fn parse_repost_inner(raw: &str) -> (Option<String>, Option<String>) {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') {
        return (None, None);
    }
    let value: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return (None, None),
    };
    let inner_id = value.get("id").and_then(|v| v.as_str()).map(str::to_owned);
    let inner_content = value
        .get("content")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    (inner_id, inner_content)
}

#[cfg(test)]
mod repost_inner_tests {
    use super::parse_repost_inner;

    #[test]
    fn empty_content_returns_none() {
        assert_eq!(parse_repost_inner(""), (None, None));
    }

    #[test]
    fn non_object_content_returns_none() {
        // NIP-18 reposts MAY ship empty `content`; Twitter-style "RT @…" plain
        // text is non-protocol but seen in the wild — both fall back cleanly.
        assert_eq!(parse_repost_inner("RT some text"), (None, None));
        assert_eq!(parse_repost_inner("[1, 2, 3]"), (None, None));
        assert_eq!(parse_repost_inner("   "), (None, None));
    }

    #[test]
    fn malformed_json_returns_none() {
        assert_eq!(parse_repost_inner("{not json"), (None, None));
        assert_eq!(parse_repost_inner("{\"id\":}"), (None, None));
    }

    #[test]
    fn well_formed_inner_event_extracts_id_and_content() {
        let raw = r#"{"id":"abc123","pubkey":"def","kind":1,"content":"hello world","tags":[]}"#;
        let (id, content) = parse_repost_inner(raw);
        assert_eq!(id.as_deref(), Some("abc123"));
        assert_eq!(content.as_deref(), Some("hello world"));
    }

    #[test]
    fn partial_inner_event_only_extracts_present_fields() {
        let (id, content) = parse_repost_inner(r#"{"id":"abc","kind":1}"#);
        assert_eq!(id.as_deref(), Some("abc"));
        assert_eq!(content, None);

        let (id, content) = parse_repost_inner(r#"{"content":"hi"}"#);
        assert_eq!(id, None);
        assert_eq!(content.as_deref(), Some("hi"));
    }

    #[test]
    fn non_string_id_or_content_falls_back_to_none() {
        // A relay sending a numeric `id` field is malformed per NIP-01; the
        // extractor must not panic and must not coerce — we degrade silently.
        let (id, content) = parse_repost_inner(r#"{"id":42,"content":null}"#);
        assert_eq!(id, None);
        assert_eq!(content, None);
    }

    #[test]
    fn leading_whitespace_is_tolerated() {
        let raw = "  \n  {\"id\":\"x\",\"content\":\"y\"}";
        let (id, content) = parse_repost_inner(raw);
        assert_eq!(id.as_deref(), Some("x"));
        assert_eq!(content.as_deref(), Some("y"));
    }
}

/// Pluralized affordance label for the "Show N earlier" header above the
/// focused thread item. Empty when `count == 0` so the host renders nothing
/// without a branch (host renders `Text(label)` unconditionally; an empty
/// string collapses to a no-op). Plain English form — see aim.md §6
/// anti-pattern #1: native must not duplicate pluralization.
/// `true` when `s` is exactly 64 lowercase hex characters — the canonical
/// form of a Nostr event id. Used by `lookup_for_primary_id` to choose
/// between a direct `events.get` lookup (event-id-form `primary_id`) and
/// the coordinate scan (`kind:pubkey:d_tag` form). Coordinate-form
/// strings never match (kind digits ≤ 5 chars, then `:`, then a 64-hex
/// pubkey, etc. — total length differs from 64 in every legal case).
fn is_hex64_lower(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn format_previous_count_label(count: usize) -> String {
    match count {
        0 => String::new(),
        1 => "Show 1 earlier note".to_string(),
        n => format!("Show {n} earlier notes"),
    }
}

/// Pluralized affordance label for the "N more replies" footer below the
/// focused thread item. Empty when `count == 0`. Same rationale as
/// [`format_previous_count_label`].
fn format_next_count_label(count: usize) -> String {
    match count {
        0 => String::new(),
        1 => "1 more reply".to_string(),
        n => format!("{n} more replies"),
    }
}

#[cfg(test)]
mod thread_label_tests {
    use super::{format_next_count_label, format_previous_count_label};

    #[test]
    fn previous_count_label_pluralizes_correctly() {
        assert_eq!(format_previous_count_label(0), "");
        assert_eq!(format_previous_count_label(1), "Show 1 earlier note");
        assert_eq!(format_previous_count_label(2), "Show 2 earlier notes");
        assert_eq!(format_previous_count_label(42), "Show 42 earlier notes");
    }

    #[test]
    fn next_count_label_pluralizes_correctly() {
        assert_eq!(format_next_count_label(0), "");
        assert_eq!(format_next_count_label(1), "1 more reply");
        assert_eq!(format_next_count_label(2), "2 more replies");
        assert_eq!(format_next_count_label(99), "99 more replies");
    }
}
