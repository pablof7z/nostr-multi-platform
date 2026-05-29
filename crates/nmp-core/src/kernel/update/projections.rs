use super::super::{ClaimedEventDto, Kernel, SettingsHubSummary, TimelineItem};

impl Kernel {
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
    /// `"settings_hub"`, `"accounts"`, `"active_account"`, or `"profile"` is
    /// overwritten so the kernel-owned value stays authoritative.
    ///
    /// D5: view-dependent keys (`timeline`, `inserted`, `updated`, `removed`,
    /// `author_view`, `thread_view`) are only inserted when the corresponding
    /// view is open — they do NOT cross the language boundary when no view is
    /// subscribed. All shells decode them as Optional with appropriate defaults.
    /// A serialization failure degrades to a stable empty value (`[]` for the
    /// lists, `null` for the optional payloads) — D6: never a panic at the
    /// snapshot boundary.
    pub(super) fn snapshot_projections_with_publish_cluster(
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
        // `Null` -> not inserted) and a `[{correlation_id, status, error}, ...]`
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
        // before acking. Absent in steady state (`Null` -> not inserted).
        let action_stages = self.action_stages_projection();
        if !action_stages.is_null() {
            projections.insert("action_stages".to_string(), action_stages);
        }
        // V5 thin-shell display projection. `action_lifecycle` collapses the
        // per-stage history `action_stages` carries into the host's
        // `{in_flight, recent_terminal}` shape, with TTL-based eviction of
        // terminals (no host ack required). Absent in steady state — same
        // `Null -> omit key` convention as `action_results` / `action_stages`.
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
        // D0: views cluster. `profile` is the active-account profile card.
        // The remaining view-dependent keys are bounded by D5: they cross the
        // language boundary only when the corresponding view is actually open.
        //
        // `timeline` / `inserted` / `updated` / `removed`: present only when
        // the shell has called `nmp_app_open_timeline` (i.e.
        // `follow_feed_kinds` is non-empty). The shell sets
        // `follow_feed_kinds` via `ActorCommand::OpenContactListSubscription`
        // and never reads these keys before that call — every shell decodes
        // them as Optional with a `[]` default (iOS: `?? []`, Kotlin:
        // `= emptyList()`, web: `Array.isArray(...) ? ... : []`).
        //
        // `author_view` / `thread_view`: present only when the respective
        // view is open (their return values are already `Option<_>`; we skip
        // inserting the key entirely rather than inserting JSON `null`). All
        // shells decode these as Optional and handle `None` / absent gracefully.
        //
        // Serialization failures degrade to empty arrays or `null` as before —
        // D6: never a panic at the snapshot boundary.
        projections.insert(
            "profile".to_string(),
            serde_json::to_value(self.profile_card()).unwrap_or(serde_json::Value::Null),
        );
        // D5: timeline cluster — only cross the boundary when the shell has
        // subscribed to the follow feed via `nmp_app_open_timeline`.
        if !self.follow_feed_kinds.is_empty() {
            projections.insert(
                "timeline".to_string(),
                serde_json::to_value(items)
                    .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
            );
            projections.insert(
                "inserted".to_string(),
                serde_json::to_value(inserted)
                    .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
            );
            projections.insert(
                "updated".to_string(),
                serde_json::to_value(updated)
                    .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
            );
            projections.insert(
                "removed".to_string(),
                serde_json::to_value(removed)
                    .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
            );
        }
        // D5: author_view / thread_view — only insert when the view is open.
        if let Some(author_view) = self.author_view() {
            projections.insert(
                "author_view".to_string(),
                serde_json::to_value(author_view).unwrap_or(serde_json::Value::Null),
            );
        }
        if let Some(thread_view) = self.thread_view() {
            projections.insert(
                "thread_view".to_string(),
                serde_json::to_value(thread_view).unwrap_or(serde_json::Value::Null),
            );
        }
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
        // `visible_items()` — empty when the timeline view is not open), the
        // open `author_view` items, and the open `thread_view` items so
        // HomeFeedView / ThreadScreen / ProfileView all find their authors
        // pre-mapped without reconstructing the dict in Swift (V-31
        // thin-shell; replaces the Swift Dictionary derivations at
        // `HomeFeedView.swift:187-197` and `ThreadScreen.swift:23-35`).
        // First writer wins on collision — matches
        // `mention_profiles_from_items` semantics. Empty `{}` when no events
        // are visible and no view is open; never absent (D1).
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
        // `claimed_profiles` projection — keyed by pubkey for every currently
        // claimed UI profile. This is the reference-first component path:
        // native registry components call `claim_profile(pubkey, consumer)`,
        // the kernel owns relay/cache policy, and the next snapshot exposes the
        // claimed profile card here. Missing kind:0 data still emits a
        // placeholder card so components can render an honest fallback
        // immediately and refine in place when the profile arrives.
        let mut claimed_profiles: std::collections::BTreeMap<String, _> =
            std::collections::BTreeMap::new();
        for pubkey in self.profile_claims.keys() {
            let npub = crate::display::to_npub(pubkey);
            claimed_profiles.insert(
                pubkey.clone(),
                self.profile_card_for(pubkey, Some(&npub), ""),
            );
        }
        projections.insert(
            "claimed_profiles".to_string(),
            serde_json::to_value(&claimed_profiles)
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
                // Enrich with the author's display name + picture URL from
                // the kernel's profile cache so the embed renderer can
                // compose with NostrProfileName / NostrAvatar without
                // having to make a separate FFI claim_profile round-trip.
                // `None` when no kind:0 has been ingested for the author —
                // the renderer falls back to truncated npub + identicon
                // until the profile arrives in a later snapshot tick.
                let profile = self.profile_for_pubkey(&stored.author);
                let display_name = profile
                    .map(|p| p.display.clone())
                    .filter(|d| !d.trim().is_empty());
                let picture_url = profile.and_then(|p| p.picture_url.clone());
                claimed_events.insert(
                    key.clone(),
                    ClaimedEventDto::from_stored(key.clone(), &stored)
                        .with_author_profile(display_name, picture_url),
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
}
