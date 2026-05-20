use super::*;
use crate::substrate::placeholder::picture_placeholder;

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
    pub(crate) fn make_update(&mut self, running: bool) -> String {
        let emit_started = Instant::now();
        // Wall-clock stamp for the actor-thread liveness heartbeat. `Instant`
        // above is monotonic and cannot be compared to a shell-side clock, so
        // a separate `SystemTime` reading is required. `unwrap_or_default()`
        // (not `unwrap()`) keeps this off the panic path (D6: no panic at the
        // public boundary) — a pre-1970 clock simply yields `0`.
        let last_tick_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
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

        let visible_profiled_items = items
            .iter()
            .filter(|item| item.author_avatar_source == "kind0")
            .count();
        let visible_placeholder_avatar_items = items.len().saturating_sub(visible_profiled_items);
        let counters = self.total_counters();
        let update = KernelSnapshot {
            rev: self.rev,
            schema_version: KERNEL_SCHEMA_VERSION,
            last_tick_ms,
            update_kind: "ViewBatch",
            running,
            relay_url: "",
            test_npub: "",
            profile: self.profile_card(),
            items,
            author_view: self.author_view(),
            thread_view: self.thread_view(),
            inserted: inserted.clone(),
            updated: updated.clone(),
            removed: removed.clone(),
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
                actor_queue_depth: 0,
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
                // T114b — per-dispatch retention audit visibility (PD-021 line-11).
                dispatch_drops_total: self.dispatch_drops_total(),
                claim_drops_total: self.claim_drops_total(),
            },
            relay_status: self.relay_status(),
            relay_statuses: self.relay_statuses(),
            logical_interests: self.logical_interests(),
            wire_subscriptions: self.wire_subscriptions(),
            logs: self.logs.iter().cloned().collect(),
            accounts: self.account_snapshot().0.to_vec(),
            active_account: self.account_snapshot().1.cloned(),
            publish_queue: self.publish_queue_snapshot().to_vec(),
            publish_outbox: self.publish_outbox_items(),
            last_error_toast: self.last_error_toast_snapshot().cloned(),
            last_error_category: self.last_error_category_snapshot().cloned(),
            // #171 (D6): project the recorded planner error so the host can
            // observe a genuine structural compile failure instead of silent
            // empty frames. `None` (→ JSON null) in steady state.
            last_planner_error: self.lifecycle.last_planner_error().map(str::to_owned),
            relay_edit_rows: self.relay_edit_rows_snapshot().to_vec(),
            #[cfg(feature = "wallet")]
            wallet_status: self.wallet_status_snapshot().cloned(),
            bunker_handshake: self.bunker_handshake_snapshot().cloned(),
        };

        // Serialize the snapshot exactly once. The on-wire `payload_bytes`
        // metric above already reflects the previous tick's size; the perf log
        // below uses this tick's true length so the diagnostic stays accurate.
        let serialized = serde_json::to_string(&update).unwrap_or_else(|_| "{}".to_string());
        if batch_events > 0 || !inserted.is_empty() || !updated.is_empty() || !removed.is_empty() {
            self.log(format!(
                "NMP_PERF rust_update rev={} batch_events={} inserted={} updated={} removed={} visible={} payload_bytes={} event_to_emit_ms={} max_event_to_emit_ms={}",
                self.rev,
                batch_events,
                inserted.len(),
                updated.len(),
                removed.len(),
                self.last_emitted_items.len(),
                serialized.len(),
                last_event_to_emit_ms
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                self.max_event_to_emit_ms
            ));
        }
        self.events_since_last_update = 0;
        self.changed_since_emit = false;
        // Remember this tick's size so the next tick's `payload_bytes` metric
        // can be set without a throwaway serialize.
        self.last_payload_bytes = serialized.len();
        serialized
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
        // D1: author_picture_url is always non-empty.  Use the kind:0 URL when
        // available; fall back to a deterministic identicon URI otherwise.
        // ADR-0017: the source discriminator MUST track the same selection the
        // URL did — a profile that exists but carries no picture still resolves
        // to the placeholder, so it is reported as `placeholder`, not `kind0`.
        let real_picture = profile
            .and_then(|p| p.picture_url.as_deref())
            .filter(|url| !url.is_empty());
        let author_picture_url = real_picture
            .map(str::to_owned)
            .unwrap_or_else(|| picture_placeholder(&event.author));
        TimelineItem {
            id: event.id.clone(),
            author_pubkey: event.author.clone(),
            author_display: profile
                .map(|profile| profile.display.clone())
                .filter(|display| !display.is_empty())
                .unwrap_or_else(|| short_pubkey_display(&event.author)),
            author_picture_url,
            author_avatar_initials: profile
                .map(|profile| profile.avatar_initials.clone())
                .unwrap_or_else(|| "..".to_string()),
            author_avatar_color: profile
                .map(|profile| profile.avatar_color.clone())
                .unwrap_or_else(|| avatar_color(&event.author)),
            author_avatar_source: if real_picture.is_some() {
                "kind0".to_string()
            } else {
                "placeholder".to_string()
            },
            content: truncate(&event.content, 1_200),
            content_preview: if event.kind == 6 && event.content.trim().is_empty() {
                "Repost".to_string()
            } else {
                truncate(&event.content.replace('\n', " "), 180)
            },
            created_at_display: format_timestamp(event.created_at),
            relay_count: event.relay_count,
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
        // D1: picture_url is always non-empty.  Use the kind:0 URL when
        // available; fall back to a deterministic identicon URI otherwise.
        // ADR-0017: `source` MUST track the same selection the URL did.
        let real_picture = profile
            .and_then(|p| p.picture_url.as_deref())
            .filter(|url| !url.is_empty());
        let picture_url = real_picture
            .map(str::to_owned)
            .unwrap_or_else(|| picture_placeholder(pubkey));
        ProfileCard {
            pubkey: pubkey.to_string(),
            npub: npub.unwrap_or(pubkey).to_string(),
            display: profile
                .map(|profile| profile.display.clone())
                .filter(|display| !display.is_empty())
                .unwrap_or_else(|| short_pubkey_display(pubkey)),
            picture_url,
            nip05: profile
                .map(|profile| profile.nip05.clone())
                .unwrap_or_default(),
            about: profile
                .map(|profile| truncate(&profile.about.replace('\n', " "), 220))
                .unwrap_or_else(|| placeholder_about.to_string()),
            avatar_initials: profile
                .map(|profile| profile.avatar_initials.clone())
                .unwrap_or_else(|| "..".to_string()),
            avatar_color: profile
                .map(|profile| profile.avatar_color.clone())
                .unwrap_or_else(|| avatar_color(pubkey)),
            source: if real_picture.is_some() {
                "kind0".to_string()
            } else {
                "placeholder".to_string()
            },
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
            return Some(ProfileAction {
                kind: "edit_profile",
                label: "Edit",
                target_pubkey: pubkey.to_string(),
            });
        }

        let is_following = self
            .seed_contacts
            .get(active)
            .map(|follows| follows.iter().any(|follow| follow == pubkey))
            .unwrap_or(false);
        let (kind, label) = if is_following {
            ("unfollow", "Unfollow")
        } else {
            ("follow", "Follow")
        };
        Some(ProfileAction {
            kind,
            label,
            target_pubkey: pubkey.to_string(),
        })
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

        Some(AuthorViewPayload {
            pubkey: pubkey.clone(),
            state: state.to_string(),
            profile: self.profile_card_for(pubkey, None, "Waiting for selected author kind:0"),
            note_count: items.len(),
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
        let next_count = focused_index
            .map(|index| items.len().saturating_sub(index + 1))
            .unwrap_or(0);
        let state = if self.thread_view.request_pending {
            "queued"
        } else if items.is_empty() {
            "opening"
        } else {
            "ready"
        };

        Some(ThreadViewPayload {
            focused_event_id: focused_id.clone(),
            root_event_id: root_id,
            state: state.to_string(),
            items,
            previous_count,
            next_count,
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
}
