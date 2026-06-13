use super::super::{ClaimedEventDto, Kernel, MentionProfilePayload, ProfileCard};

/// Canonical list of the kernel-owned (Tier-2) built-in projection keys —
/// every key [`Kernel::snapshot_projections_with_publish_cluster`] can insert
/// into the snapshot `projections` map (the registry-closure Tier-1 keys are
/// NOT listed here; they are introspectable via the live `SnapshotRegistry`).
///
/// This is the single source of truth for "which projection keys does the
/// kernel itself produce". Two consumers depend on it staying exact:
///
/// 1. The registry-coverage gate
///    (`nmp-app-chirp::ffi::tests::producer_completeness::every_codegen_registry_key_is_registered_at_runtime`)
///    asserts every `nmp-codegen` `SNAPSHOT_PROJECTIONS` json_key is either a
///    runtime-registered Tier-1 closure key or a member of this list — closing
///    the #1084-class hole where a producer-side key rename ships without its
///    consumers (the codegen registry, the Swift/Kotlin bridges).
/// 2. The in-crate pinning test
///    (`builtin_projection_keys_const_matches_runtime`) drives a real
///    `make_update` tick and asserts the emitted built-in keys are a subset of
///    this list AND that every unconditional key is present — so the const
///    cannot silently drift from the insertion code above.
///
/// Conditional keys (`action_results` / `signed_events` / `action_stages` /
/// `action_lifecycle` — drain-on-emit, present only on ticks where something
/// settled) are listed too: the gate asks "can the kernel produce this key",
/// not "is it present this tick".
pub const KERNEL_BUILTIN_PROJECTION_KEYS: &[&str] = &[
    "publish_queue",
    "publish_outbox",
    "outbox_summary",
    "configured_relays",
    "relay_role_options",
    "settings_hub",
    "action_results",
    "signed_events",
    "action_stages",
    "action_lifecycle",
    "accounts",
    "active_account",
    "profile",
    "relay_diagnostics",
    "mention_profiles",
    "claimed_profiles",
    "claimed_events",
    "resolved_profiles",
];

impl Kernel {
    /// Collect the snapshot `projections` map: every host-registered
    /// projection closure plus the kernel-owned built-in projections (the
    /// publish / relay-settings cluster, the identity pair, and the views cluster).
    ///
    /// D0: `publish_queue`, `publish_outbox`, `configured_relays`, and
    /// `relay_role_options` are app-shaped relay/publish state; `accounts` /
    /// `active_account` are identity output; and the views cluster (`profile`,
    /// `author_view`, `thread_view`) is app-shaped social view state — none are
    /// protocol-neutral kernel primitives, so none carry a typed
    /// `KernelSnapshot` field. Unlike the host-registered `"wallet"` /
    /// `"bunker_handshake"` projections (which read actor-runtime slots through
    /// a no-arg closure), these are kernel-owned, so they cannot be expressed as
    /// a `SnapshotRegistry` closure — they are inserted here directly after the
    /// host closures run.
    ///
    /// `profile_card()`, `author_view()`, and `thread_view()` read `&self` and
    /// are called inside this helper.
    ///
    /// Step 3A (issue #920): the follow-feed projection cluster (`timeline` /
    /// `inserted` / `updated` / `removed`) has been removed. The kernel no
    /// longer derives a per-tick `visible_items()` list, so those keys are no
    /// longer inserted and `mention_profiles` is seeded only from the open
    /// `author_view` / `thread_view` items.
    ///
    /// Built-in keys win on collision: a host that registers `"publish_queue"`,
    /// `"publish_outbox"`, `"configured_relays"`, `"relay_role_options"`,
    /// `"settings_hub"`, `"accounts"`, `"active_account"`, or `"profile"` is
    /// overwritten so the kernel-owned value stays authoritative.
    ///
    /// D5: view-dependent keys (`author_view`, `thread_view`) are only inserted
    /// when the corresponding view is open — they do NOT cross the language
    /// boundary when no view is subscribed. All shells decode them as Optional
    /// with appropriate defaults. A serialization failure degrades to a stable
    /// empty value (`null` for the optional payloads) — D6: never a panic at the
    /// snapshot boundary.
    pub(super) fn snapshot_projections_with_publish_cluster(
        &mut self,
    ) -> std::collections::HashMap<String, serde_json::Value> {
        let mut projections = self.run_snapshot_projections();
        // ADR-0053 — snapshot the host-declared consumed-projection set ONCE
        // this tick. `permits(key)` returns `true` for every key when the set is
        // empty (no narrowing) and only for declared members otherwise. Each
        // Tier-2 built-in below is gated on it; a gated-out key skips its
        // producer entirely (no serialize, no roll-up). The captured-value
        // built-ins (`action_*`, `relay_diagnostics`) gate at the capture site,
        // so the Tier-2 typed sidecar path naturally omits them (its `if let
        // Some(..)` sees `None`) — keeping the ADR-0037 JSON/typed parity.
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
                serde_json::to_value(self.publish_outbox_items()).unwrap_or(serde_json::Value::Null),
            );
        }
        // D0: outbox header summary — `OutboxSummarySnapshot`. The kernel owns
        // the per-status counters AND the English `title` / `subtitle`
        // strings (§6 anti-pattern #1); shells bind the strings verbatim
        // instead of `.filter`-counting `publish_outbox` to derive them.
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
        // Settings-hub view projection. Raw relay count — the host formats any
        // pluralization string (aim.md §6/AP1 forbids the shell from owning that).
        if declared.permits("settings_hub") {
            projections.insert(
                "settings_hub".to_string(),
                serde_json::json!({ "relay_count": self.configured_relays_snapshot().len() }),
            );
        }
        // Direction review #29: drain EVERY terminal that settled since the
        // last emit into the `action_results` array. The host can clear a
        // per-action spinner (published / failed / cancelled) without polling.
        // If two actions settled in the same tick the host sees both, so no
        // spinner hangs. This key is absent in steady state (drain returns
        // `Null` -> not inserted) and a `[{correlation_id, status, error}, ...]`
        // array whenever any action settled this tick. The host resolves each
        // spinner by correlation_id.
        // ADR-0053: ALWAYS drain (draining keeps the source bounded; the
        // declared set is static, so an undeclared `action_results` would never
        // be consumed and discarding it is correct). Capture + insert only when
        // declared — gating the capture makes the Tier-2 typed sidecar omit it
        // too (its `if let Some(..)` sees `None`).
        let action_results = self.take_action_results_projection();
        let emit_action_results = declared.permits("action_results") && !action_results.is_null();
        // Wave C (ADR-0037): capture the DRAINED value ONCE this tick so the
        // Tier-2 typed sidecar (`builtin_typed_projections`) encodes from the
        // exact same array WITHOUT re-invoking the draining accessor.
        // `Some` iff the JSON key is inserted below; `None` resets any prior
        // tick's capture (present-iff-present, no stale carryover).
        self.captured_action_results = emit_action_results.then(|| action_results.clone());
        if emit_action_results {
            projections.insert("action_results".to_string(), action_results);
        }
        // D13 sign-and-return: drain every `SignEventForReturn` result that
        // settled since the last emit into the `signed_events` projection,
        // keyed by `correlation_id`. The host's `signEventForReturn`
        // continuation resumes the moment its id appears here. Same
        // `Null -> omit key` + drain-once convention as `action_results`; the
        // host reads each id exactly once. Absent in steady state.
        // ADR-0053: always drain; capture + insert only when declared.
        let signed_events = self.take_signed_events_projection();
        let emit_signed_events = declared.permits("signed_events") && !signed_events.is_null();
        // Wave C: capture the DRAINED value once (see `action_results` above).
        self.captured_signed_events = emit_signed_events.then(|| signed_events.clone());
        if emit_signed_events {
            projections.insert("signed_events".to_string(), signed_events);
        }
        // Snapshot mirror of every in-flight action's lifecycle stages,
        // keyed by `correlation_id`. Unlike `action_results` (drain on emit),
        // `action_stages` is a *copy* — the same correlation_id reappears on
        // every tick until the host calls `nmp_app_ack_action_stage`. The host
        // renders a progress indicator from the latest stage in each id's
        // history and clears it on the terminal stage (`Accepted` / `Failed`)
        // before acking. Absent in steady state (`Null` -> not inserted).
        let action_stages = self.action_stages_projection();
        let emit_action_stages = declared.permits("action_stages") && !action_stages.is_null();
        // Wave C: capture once. This accessor is a `&self` COPY, but the
        // `action_results` drain above records terminals into this mirror within
        // the same tick, so capturing here reads the exact value the JSON key
        // carries (uniform with the four genuinely-drained built-ins).
        self.captured_action_stages = emit_action_stages.then(|| action_stages.clone());
        if emit_action_stages {
            projections.insert("action_stages".to_string(), action_stages);
        }
        // V5 thin-shell display projection. `action_lifecycle` collapses the
        // per-stage history `action_stages` carries into the host's
        // `{in_flight, recent_terminal}` shape, with TTL-based eviction of
        // terminals (no host ack required). Absent in steady state — same
        // `Null -> omit key` convention as `action_results` / `action_stages`.
        // The mutable borrow runs the tracker's TTL sweep on every emit so a
        // quiet kernel still prunes expired terminals. ADR-0053: the accessor is
        // always called (the TTL sweep must run regardless of declaration);
        // capture + insert only when declared.
        let action_lifecycle = self.action_lifecycle_projection();
        let emit_action_lifecycle =
            declared.permits("action_lifecycle") && !action_lifecycle.is_null();
        // Wave C: capture once. `action_lifecycle_projection()` is `&mut self`
        // (it runs the TTL sweep), so it MUST NOT be re-invoked for the typed
        // path — capture the produced value here instead.
        self.captured_action_lifecycle = emit_action_lifecycle.then(|| action_lifecycle.clone());
        if emit_action_lifecycle {
            projections.insert("action_lifecycle".to_string(), action_lifecycle);
        }
        // D0: identity output. `accounts_enriched()` returns `AccountSummary`
        // records patched with kind:0 picture_url / display_name so the toolbar
        // avatar and accounts list show real profile data. `active_account` is
        // still sourced from the raw snapshot (it is just a pubkey string).
        // ADR-0053: `accounts_enriched()` (a kind:0 patch over the account list)
        // runs only when `accounts` is declared.
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
        // D0: views cluster. `profile` is the active-account profile card.
        // The remaining view-dependent keys are bounded by D5: they cross the
        // language boundary only when the corresponding view is actually open.
        //
        // Step 3A (issue #920): the follow-feed cluster (`timeline` /
        // `inserted` / `updated` / `removed`) is no longer produced here — the
        // kernel no longer derives a per-tick `visible_items()` list.
        //
        // `author_view` / `thread_view`: present only when the respective
        // view is open (their return values are already `Option<_>`; we skip
        // inserting the key entirely rather than inserting JSON `null`). All
        // shells decode these as Optional and handle `None` / absent gracefully.
        //
        // Serialization failures degrade to `null` as before —
        // D6: never a panic at the snapshot boundary.
        if declared.permits("profile") {
            projections.insert(
                "profile".to_string(),
                serde_json::to_value(self.profile_card()).unwrap_or(serde_json::Value::Null),
            );
        }
        // V-112 (ADR-0042): author_view / thread_view projection inserts deleted.
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
        // Wave C: build the diagnostics roll-up ONCE this tick. The accessor
        // pre-formats wall-clock-relative "Xs ago" labels against an internal
        // `now`, so calling it a second time for the typed sidecar could
        // straddle a one-second bucket and diverge from this JSON form. The JSON
        // is serialised from the captured struct; the typed path
        // (`builtin_typed_projections`) maps the SAME captured instance.
        //
        // ADR-0053 — THE headline gate. `relay_diagnostics_snapshot()` is the
        // most expensive Tier-2 roll-up (every relay row + wire-sub + relative
        // labels). When the host does not declare `"relay_diagnostics"` the whole
        // roll-up is skipped, the JSON key is omitted, and `captured_*` stays
        // `None` so the typed sidecar omits it too. A debug-only diagnostics
        // screen no longer costs every host serialize+encode+decode 4×/sec.
        if declared.permits("relay_diagnostics") {
            let relay_diagnostics = self.relay_diagnostics_snapshot();
            projections.insert(
                "relay_diagnostics".to_string(),
                serde_json::to_value(&relay_diagnostics).unwrap_or(serde_json::Value::Null),
            );
            self.captured_relay_diagnostics = Some(relay_diagnostics);
        } else {
            // Reset any prior tick's capture so the typed sidecar omits it.
            self.captured_relay_diagnostics = None;
        }
        // `mention_profiles` — derived view (aim.md §4.2): pubkey ->
        // {display, picture_url, avatar_initials, avatar_color} for every
        // author surfaced in ANY currently-open view. Built from the union of
        // the open `author_view` items and the open `thread_view` items so
        // ThreadScreen / ProfileView find their authors pre-mapped without
        // reconstructing the dict in Swift (V-31 thin-shell; replaces the Swift
        // Dictionary derivation at `ThreadScreen.swift:23-35`).
        //
        // Step 3A (issue #920): the home `timeline` contribution (formerly the
        // `visible_items()` `items` argument) has been removed — `mention_profiles`
        // is now seeded only from the open author/thread views. First writer wins
        // on collision — matches `mention_profiles_from_items` semantics. Empty
        // `{}` when no view is open; never absent (D1).
        if declared.permits("mention_profiles") {
            let mention_profiles = self.mention_profiles();
            projections.insert(
                "mention_profiles".to_string(),
                serde_json::to_value(&mention_profiles)
                    .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default())),
            );
        }
        // `claimed_profiles` projection — keyed by pubkey for every currently
        // claimed UI profile. This is the reference-first component path:
        // native registry components call `claim_profile(pubkey, consumer)`,
        // the kernel owns relay/cache policy, and the next snapshot exposes the
        // claimed profile card here. Missing kind:0 data still emits a
        // placeholder card so components can render an honest fallback
        // immediately and refine in place when the profile arrives.
        if declared.permits("claimed_profiles") {
            let claimed_profiles = self.claimed_profiles();
            projections.insert(
                "claimed_profiles".to_string(),
                serde_json::to_value(&claimed_profiles)
                    .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default())),
            );
        }
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
        if declared.permits("claimed_events") {
            let claimed_events = self.claimed_events();
            projections.insert(
                "claimed_events".to_string(),
                serde_json::to_value(&claimed_events)
                    .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default())),
            );
        }
        // `resolved_profiles` — pre-merged profile map for all consumers.
        // Precedence: claimed_profiles (highest) → author_view.profile
        // (only-if-absent) → mention_profiles (only-if-absent). Shipping the
        // merge once here lets every shell delete its per-platform merge code
        // (e.g. the TUI `LiveProfileMap` three-step ingest) and just read this
        // key. Always present as `{}` when empty (D1); BTreeMap for
        // deterministic key ordering (snapshot diff stability), mirroring
        // `claimed_profiles` / `claimed_events`.
        if declared.permits("resolved_profiles") {
            let resolved_profiles = self.resolved_profiles();
            projections.insert(
                "resolved_profiles".to_string(),
                serde_json::to_value(&resolved_profiles)
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
    /// best-effort). Each entry is enriched with the author's cached kind:0
    /// display name + picture URL so the embed renderer composes with
    /// NostrProfileName / NostrAvatar without a separate claim round-trip.
    /// BTreeMap for deterministic key ordering.
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

        // V-112 (ADR-0042): author_view.profile source deleted. Profile data for
        // the author screen is now resolved via claimed_profiles (claim_profile
        // from nmp_app_chirp_open_author_feed).

        // 3. mention_profiles — only-if-absent (lowest precedence).
        for (pubkey, m) in self.mention_profiles() {
            resolved
                .entry(pubkey.clone())
                .or_insert_with(|| ProfileCard::from_mention(&pubkey, &m));
        }

        resolved
    }
}
