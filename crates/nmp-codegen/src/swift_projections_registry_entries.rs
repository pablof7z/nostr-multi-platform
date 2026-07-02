//! Owns the concrete `SNAPSHOT_PROJECTIONS` registry rows — the dotted-
//! projection-key catalog itself, in declaration order (order is
//! load-bearing: [`crate::swift`]'s `codegen-drift` CI gate byte-diffs the
//! generated Swift, which renders one line per entry in this order).
//!
//! Split out of `swift_projections_registry.rs` (which keeps the
//! [`super::SnapshotProjectionEntry`] / [`super::TypedSidecar`] row-schema
//! definitions and the module-level maintenance contract) so the row-schema
//! file stays under the file-size ceiling.

use super::{SnapshotProjectionEntry, TypedSidecar};

/// The Stage 2 registry — every entry on the hand-written
/// `SnapshotProjections` struct in `KernelBridge.swift`, in declaration
/// order. Order is load-bearing (the generated file is byte-diffed against
/// the committed copy by the `codegen-drift` CI gate).
///
/// This slice has 31 entries (locked by `registry_size_is_locked`). Adding or
/// removing a member here changes the generated Swift — the CI gate will refuse
/// stale output until the regenerated file is committed.
///
/// #1610: removed the five JSON-era vestigial sidecar-less entries —
/// `timeline`, `inserted`, `updated`, `removed`, and `last_action_result`.
/// The coverage gate (`typed_sidecar_coverage_gate` test) now enforces that
/// every future entry carries `typed_sidecar: Some(...)` — no JSON-only slots.
pub const SNAPSHOT_PROJECTIONS: &[SnapshotProjectionEntry] = &[
    // Built-in NWC wallet projection. `projections["wallet"]`.
    SnapshotProjectionEntry {
        key: "wallet",
        swift_field: "wallet",
        swift_type: "WalletStatusData",
        // FLIPPED (Wave B Tier-1): the live typed `wallet` producer
        // (`apps/chirp/.../wallet_runtime.rs`:
        // `register_typed_snapshot_projection("wallet", …)` →
        // `wallet_typed_projection`) now carries the `wallet_pubkey_hex` field the
        // Swift `WalletStatusData.walletPubkeyHex` (`WalletView.swift:98`)
        // requires. The producer field-add landed it on BOTH the Rust
        // `WalletStatus` struct + the JSON projection
        // (`serde_json::to_value(WalletStatus)`) AND the `wallet_status.fbs`
        // wire (tail-appended, wire-compatible), so JSON + typed stay byte-
        // identical (no fabrication, additive parity preserved).
        //
        // NOTE on identity: the producer publishes ENVELOPE key `"wallet"`
        // (NOT `schema_id`); the decoder matches on `key == "wallet"` AND
        // `schema_id == "nmp.nip47.wallet"`. So `key` here is `"wallet"`
        // (the envelope key the producer emits) while `schema_id` stays
        // `"nmp.nip47.wallet"` — key ≠ schema_id for this entry.
        // `TypedProjectionGlue.wallet` maps the `NWST` reader to
        // `WalletStatusData`; consumed typed-first in `KernelModel.apply`
        // (`typedWallet ?? snapshot?.walletStatus`).
        typed_sidecar: Some(TypedSidecar {
            swift_reader_type: Some("nmp_nip47_WalletStatus"),
        }),
    },
    // NIP-46 bunker handshake projection. `projections["bunker_handshake"]`.
    SnapshotProjectionEntry {
        key: "bunker_handshake",
        swift_field: "bunkerHandshake",
        swift_type: "BunkerHandshake",
        // Tier-1 actor projection: producer sets `key == schema_id`.
        typed_sidecar: Some(TypedSidecar {
            // Per-key sidecar flip: the `flatc --swift` reader
            // (`nmp_kernel_BunkerHandshake`) ships with this batch from
            // `crates/nmp-core/schema/bunker_handshake.fbs`. Flat field copy with
            // the `has_message` companion → `String?` mapping; the always-present
            // wire bools surface as the domain type's forward-compat `Bool?`
            // (non-nil from the typed path). See `TypedProjectionGlue.bunkerHandshake`.
            swift_reader_type: Some("nmp_kernel_BunkerHandshake"),
        }),
    },
    // NIP-46 typed onboarding read model. Always populated by the kernel;
    // optional only so an older kernel build that predates the projection
    // still decodes (D1).
    SnapshotProjectionEntry {
        key: "nip46_onboarding",
        swift_field: "nip46Onboarding",
        swift_type: "Nip46Onboarding",
        // Tier-1 actor projection: producer sets `key == schema_id`.
        typed_sidecar: Some(TypedSidecar {
            // Per-key sidecar flip: the `flatc --swift` reader
            // (`nmp_kernel_Nip46Onboarding`, wrapping `nmp_kernel_SignerApp`) ships
            // with this batch from `crates/nmp-core/schema/nip46_onboarding.fbs`.
            // `signer_apps` nested-vector copy; the `has_stage_kind` /
            // `has_progress_message` companions → `StageKind?` / `String?`; the
            // snake_case `stage_kind` wire token re-types to the same `StageKind`
            // enum the JSON path decodes (`unknown` forward-compat fallback). See
            // `TypedProjectionGlue.nip46Onboarding`.
            swift_reader_type: Some("nmp_kernel_Nip46Onboarding"),
        }),
    },
    // Unified remote-signer health (ADR-0072 D6 — generalises the V-14 step b
    // `bunker_connection_state` projection; hard-break rename, no compat key).
    // `projections["signer_state"]` — null when no remote-signer session is
    // active; `{ signer_kind, state, reason, is_ready, is_awaiting_approval,
    // is_reconnecting, is_unavailable, is_failed }` when one is live. Covers
    // BOTH NIP-46 bunker sessions and NIP-55 external-signer (Amber) sessions;
    // shells surface ONE status badge on the active remote account row keyed
    // by `signer_kind`, and a non-blocking alert banner when degraded.
    // Tier-1 actor projection: producer sets `key == schema_id`.
    SnapshotProjectionEntry {
        key: "signer_state",
        swift_field: "signerState",
        swift_type: "SignerState",
        // Typed FlatBuffers sidecar (KSST). The `flatc --swift` reader
        // (`nmp_kernel_SignerState`) ships from
        // `crates/nmp-core/schema/signer_state.fbs`. Field-for-field copy:
        // `{ signer_kind, state, has_reason, reason, is_ready,
        // is_awaiting_approval, is_reconnecting, is_unavailable, is_failed }`.
        // Only emitted when a remote-signer session is active (the slot is
        // `Some`) — mirrors the JSON closure's `null`-while-idle behaviour.
        // See `TypedProjectionGlue.signerState`.
        typed_sidecar: Some(TypedSidecar {
            swift_reader_type: Some("nmp_kernel_SignerState"),
        }),
    },
    // Publish-cluster outbox feeds — kernel-owned `publish_queue` and
    // `publish_outbox` arrays driven by the actor publish path.
    SnapshotProjectionEntry {
        key: "publish_queue",
        swift_field: "publishQueue",
        swift_type: "[PublishQueueEntry]",
        typed_sidecar: Some(TypedSidecar {
            // Wave B batch #2: the `flatc --swift` reader
            // (`nmp_kernel_PublishQueueSnapshot`) ships in this PR. The Chirp
            // domain `[PublishQueueEntry]` is a field-SUBSET of the wire
            // (eventId/kind/targetRelays/status only); the glue maps the
            // subset (see `TypedProjectionGlue.publishQueue`).
            swift_reader_type: Some("nmp_kernel_PublishQueueSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "publish_outbox",
        swift_field: "publishOutbox",
        swift_type: "[PublishOutboxItem]",
        typed_sidecar: Some(TypedSidecar {
            // Wave B batch #2: the `flatc --swift` reader
            // (`nmp_kernel_PublishOutboxSnapshot`) ships in this PR. Field-for-
            // field copy of each item + nested `[PublishOutboxRelay]` rows; the
            // glue widens `targetRelays` (uint → Int). See
            // `TypedProjectionGlue.publishOutbox`.
            swift_reader_type: Some("nmp_kernel_PublishOutboxSnapshot"),
        }),
    },
    // §6/AP1 pre-formatted outbox header — kernel-owned strings the shell
    // renders verbatim.
    SnapshotProjectionEntry {
        key: "outbox_summary",
        swift_field: "outboxSummary",
        swift_type: "OutboxSummary",
        typed_sidecar: Some(TypedSidecar {
            // Wave B batch #2: the `flatc --swift` reader
            // (`nmp_kernel_OutboxSummarySnapshot`) ships in this PR. Single-table
            // field-for-field copy (kernel owns title/subtitle strings). See
            // `TypedProjectionGlue.outboxSummary`.
            swift_reader_type: Some("nmp_kernel_OutboxSummarySnapshot"),
        }),
    },
    // Relay-edit settings cluster — pre-rolled rows + role pick options.
    SnapshotProjectionEntry {
        key: "configured_relays",
        swift_field: "configuredRelays",
        swift_type: "[AppRelay]",
        typed_sidecar: Some(TypedSidecar {
            // Wave B batch #2: the `flatc --swift` reader
            // (`nmp_kernel_ConfiguredRelaysSnapshot`) ships in this PR. Two-field
            // (url/role) copy. See `TypedProjectionGlue.configuredRelays`.
            swift_reader_type: Some("nmp_kernel_ConfiguredRelaysSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "relay_role_options",
        swift_field: "relayRoleOptions",
        swift_type: "[RelayRoleOption]",
        typed_sidecar: Some(TypedSidecar {
            // Wave B batch #2: the `flatc --swift` reader
            // (`nmp_kernel_RelayRoleOptionsSnapshot`) ships in this PR. Two-field
            // (value/isDefault) copy. See
            // `TypedProjectionGlue.relayRoleOptions`.
            swift_reader_type: Some("nmp_kernel_RelayRoleOptionsSnapshot"),
        }),
    },
    // D0 identity output. `accounts` enriches AccountSummary rows with
    // kind:0 metadata; `active_account` is the active pubkey scalar.
    SnapshotProjectionEntry {
        key: "accounts",
        swift_field: "accounts",
        swift_type: "[AccountSummary]",
        // PROOF KEY (thin glue): the `accounts` sidecar IS emitted (Tier-2
        // built-in, `key == schema_id`) AND its `flatc --swift` reader
        // (`nmp_kernel_AccountsSnapshot`) ships in this PR. The generator emits
        // a typed decoder; the hand-written glue maps the FB rows (two
        // `has_*` companion-bool optional strings) to `[AccountSummary]`.
        typed_sidecar: Some(TypedSidecar {
            swift_reader_type: Some("nmp_kernel_AccountsSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "active_account",
        swift_field: "activeAccount",
        swift_type: "String",
        // PROOF KEY (thinnest glue): the `active_account` sidecar IS emitted
        // (Tier-2 built-in, `key == schema_id`) AND its `flatc --swift` reader
        // (`nmp_kernel_ActiveAccountSnapshot`) ships in this PR. The glue is a
        // single line — `fb.hasActiveAccount ? fb.pubkey : nil` — mapping the
        // companion-bool to the optional `String` the JSON path yields.
        typed_sidecar: Some(TypedSidecar {
            swift_reader_type: Some("nmp_kernel_ActiveAccountSnapshot"),
        }),
    },
    // Action lifecycle cluster — see kernel/update.rs::snapshot_projections_with_publish_cluster.
    // `action_results` is a per-tick drain; `action_stages` is the
    // per-correlation_id stage mirror; `action_lifecycle` is the V5
    // collapsed view (`in_flight` + `recent_terminal` w/ TTL eviction).
    // `last_action_result` (the deprecated sticky scalar) was removed in #1610:
    // `action_results` is the canonical typed source.
    SnapshotProjectionEntry {
        key: "action_results",
        swift_field: "actionResults",
        swift_type: "[LastActionResult]",
        typed_sidecar: Some(TypedSidecar {
            // Wave C: `flatc --swift` reader (`nmp_kernel_ActionResultsSnapshot`)
            // generated in this PR. Each `ActionResult` row maps
            // `correlation_id`, `status`, `has_error`/`error` to the existing
            // `LastActionResult` Swift type. See `TypedProjectionGlue.actionResults`.
            swift_reader_type: Some("nmp_kernel_ActionResultsSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "action_stages",
        swift_field: "actionStages",
        swift_type: "[String: [ActionStageEntry]]",
        typed_sidecar: Some(TypedSidecar {
            // Wave C: `flatc --swift` reader (`nmp_kernel_ActionStagesSnapshot`)
            // generated in this PR. The outer snapshot carries a vector of
            // `ActionStagesEntry` (one per correlation_id), each with its own
            // `ActionStageEntry` vector. See `TypedProjectionGlue.actionStages`.
            swift_reader_type: Some("nmp_kernel_ActionStagesSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "action_lifecycle",
        swift_field: "actionLifecycle",
        swift_type: "ActionLifecycleSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // Wave B batch #3: the `flatc --swift` reader
            // (`nmp_kernel_ActionLifecycleSnapshot`) ships in this PR. The
            // `{ in_flight, recent_terminal }` struct maps field-for-field to
            // the Chirp `ActionLifecycleSnapshot`; each `LifecycleEntry` row
            // reconstructs the `ActionLifecycleStage` enum from
            // `stage` + `has_reason`/`reason`. See
            // `TypedProjectionGlue.actionLifecycle`.
            swift_reader_type: Some("nmp_kernel_ActionLifecycleSnapshot"),
        }),
    },
    // D0 views cluster — `profile` (typed). V-112 (ADR-0076): author_view /
    // thread_view deleted. #1610: the JSON-era `timeline`, `inserted`,
    // `updated`, `removed` per-tick delta slots deleted. OP-feed sessions are
    // app-owned projections that decode the shared `nmp.note_feed.opfeed` /
    // NNFS schema; they are not a shared `SnapshotProjections` singleton.
    SnapshotProjectionEntry {
        key: "profile",
        swift_field: "profile",
        swift_type: "ProfileCard",
        typed_sidecar: Some(TypedSidecar {
            // Profile-cluster batch: the `flatc --swift` reader
            // (`nmp_kernel_ProfileSnapshot`, wrapping the SHARED
            // `nmp_kernel_ProfileCard` from `ProfileCard.generated.swift`) ships
            // with this batch. Single-card copy with `has_*`→`String?` companion
            // mapping. See `TypedProjectionGlue.profile`.
            swift_reader_type: Some("nmp_kernel_ProfileSnapshot"),
        }),
    },
    // Host-registered dotted-key projections. The `.` in the JSON key is
    // opaque to `.convertFromSnakeCase` (it splits on `_` only), so the
    // post-transform key keeps the `nmp.<nip>.<verb>` shape but with the
    // tail camelised.
    SnapshotProjectionEntry {
        key: "nmp.nip29.group_events",
        swift_field: "groupEvents",
        swift_type: "GroupEventsSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // Wave B Tier-1 #4: the `flatc --swift` reader
            // (`nmp_nip29_GroupEventsSnapshot`) ships in this PR. Host-registered
            // producer is the NIP-29 group-events typed read session (#2187):
            // descriptor open → `register_typed_snapshot_projection`.
            // Flat field-for-field copy: `{ events: [GroupEvent] }`,
            // each row `{ id, pubkey, content, created_at, kind }`. See
            // `TypedProjectionGlue.groupEvents`.
            swift_reader_type: Some("nmp_nip29_GroupEventsSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "nmp.nip17.dm_inbox",
        swift_field: "dmInbox",
        swift_type: "DmInboxSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // NIP-17 DM cluster batch: the `flatc --swift` reader
            // (`nmp_nip17_DmInboxSnapshot`, wrapping `nmp_nip17_DmConversation` /
            // `nmp_nip17_DmMessage`) ships with this batch from
            // `crates/nmp-nip17/schema/dm_inbox.fbs`. Conversations/messages
            // nested-vector copy preserving the Rust newest-first order. See
            // `TypedProjectionGlue.dmInbox`.
            swift_reader_type: Some("nmp_nip17_DmInboxSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "nmp.follow_list",
        swift_field: "followList",
        swift_type: "FollowListSnapshot",
        // The registry/projection key (`nmp.follow_list`) differs from the
        // buffer's `schema_id` (`nmp.nip02.follow_list`); verify the producer's
        // actual `(key, schema_id)` push before generating its decoder.
        typed_sidecar: Some(TypedSidecar {
            // Wave B Tier-1 #4: the `flatc --swift` reader
            // (`nmp_nip02_FollowListSnapshot`) ships in this PR. Host-registered
            // producer in `apps/chirp/.../ffi/register.rs`
            // (`register_typed_snapshot_projection("nmp.follow_list", …)` →
            // `follow_list_typed_projection`). Note the deliberate key/schema_id
            // split: the envelope KEY is `nmp.follow_list`, the payload
            // SCHEMA_ID is `nmp.nip02.follow_list` — the generated decoder
            // matches on BOTH (verified against the producer). Flat copy:
            // `{ follows: [FollowEntry{pubkey}] }`. See
            // `TypedProjectionGlue.followList`.
            swift_reader_type: Some("nmp_nip02_FollowListSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "nmp.nip29.discovered_groups",
        swift_field: "discoveredGroups",
        swift_type: "DiscoveredGroupsSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // Wave B Tier-1 #4: the `flatc --swift` reader
            // (`nmp_nip29_DiscoveredGroupsSnapshot`) ships in this PR.
            // Host-registered producer is the NIP-29 group-discovery typed read
            // session (#2088): descriptor open → typed snapshot projection.
            // Flat copy: `{ host_relay_url, groups: [DiscoveredGroup] }`. The
            // `name`/`picture`/`about` wire strings are bare (absent == None) and
            // map to the domain's `String?` preserving nil — NOT `?? ""` — so
            // typed and JSON are byte-identical. See
            // `TypedProjectionGlue.discoveredGroups`.
            swift_reader_type: Some("nmp_nip29_DiscoveredGroupsSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "nmp.nip17.dm_relay_list",
        swift_field: "dmRelayList",
        swift_type: "DmRelayListSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // NIP-17 DM cluster batch: the `flatc --swift` reader
            // (`nmp_nip17_DmRelayListSnapshot`) ships with this batch from
            // `crates/nmp-nip17/schema/dm_relay_list.fbs`. Flat field-for-field
            // copy with `has_active_pubkey`→`String?` companion mapping. This key
            // has NO current Swift read consumer (the `dmRelayList` accessor is
            // the only seam, added for parity); flipping it is purely additive.
            // See `TypedProjectionGlue.dmRelayList`.
            swift_reader_type: Some("nmp_nip17_DmRelayListSnapshot"),
        }),
    },
    // Diagnostics roll-up + pre-merged resolved-profile map + settings hub
    // view. All single-component snake_case keys that pass through
    // `.convertFromSnakeCase` cleanly.
    SnapshotProjectionEntry {
        key: "relay_diagnostics",
        swift_field: "relayDiagnostics",
        swift_type: "RelayDiagnosticsSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // Wave B batch #3: the `flatc --swift` reader
            // (`nmp_kernel_RelayDiagnosticsSnapshot`) ships in this PR. Pure
            // field-for-field copy of the rolled-up relay rows + nested
            // wire-sub rows + logical-interest rows; every `has_*` companion
            // bool maps the optional `String?` (nil when absent). See
            // `TypedProjectionGlue.relayDiagnostics`.
            swift_reader_type: Some("nmp_kernel_RelayDiagnosticsSnapshot"),
        }),
    },
    // Pre-resolved embed-envelope map over authoritative `refs.event` rows
    // (issue #1283 / ADR-0072 §embed-sidecar) — keyed by `primary_id`, one
    // `EmbeddedEventEnvelope` (the kind-dispatched `EmbedKindProjection`) per
    // currently resolved event ref. Produced by `crates/nmp-native-runtime/src/embed_sidecar.rs`,
    // which materialises `refs.event` rows through
    // `nmp_content::resolve_embed_projection` and emits the typed `NEMB`
    // FlatBuffer (this entry, Chirp typed-frame shell). Decoding the typed
    // sidecar is what lets Chirp delete its in-Swift `match kind` embed resolver
    // (the EmbedHost D0 violation #1283 closes). The Swift value type
    // `EmbeddedEventEnvelope` is hand-declared in
    // `ios/.../Components/NostrContent/EmbedKindProjection.swift`; the glue
    // builds it from the typed reader. Drives `EmbedHost.update(envelopes:)`.
    SnapshotProjectionEntry {
        key: "refs.event.envelopes",
        swift_field: "refEventEnvelopes",
        swift_type: "[String: EmbeddedEventEnvelope]",
        typed_sidecar: Some(TypedSidecar {
            // Producer sets `key == schema_id == "refs.event.envelopes"`
            // (`embed_sidecar::install_embed_sidecar_projection`).
            // `flatc --swift` reader from `crates/nmp-content/schema/embed_sidecar.fbs`
            // (`apps/chirp/ios/Chirp/Bridge/Generated/RefEventEnvelopes.generated.swift`).
            // The `[EmbeddedEventEnvelope]` (key-sorted on `primary_id`) →
            // `[String: EmbeddedEventEnvelope]` map + the kind-discriminated
            // `EmbedKindProjection` mapping is `TypedProjectionGlue.refEventEnvelopes`.
            swift_reader_type: Some("nmp_embed_RefEventEnvelopes"),
        }),
    },
    SnapshotProjectionEntry {
        key: "settings_hub",
        swift_field: "settingsHub",
        swift_type: "[String: Int]",
        // FLIPPED: the kernel-built-in typed sidecar (`KSHB` /
        // `nmp_kernel_SettingsHubSnapshot`, Tier-2 `builtin_typed_projections`)
        // carries `relay_count:uint`, encoded from the SAME
        // `configured_relays_snapshot().len()` the JSON path reads. The FB table
        // (`{ relay_count }`) does not literally equal the Chirp domain type
        // (`[String: Int]`), so `TypedProjectionGlue.settingsHub` rebuilds the
        // single-key map `["relay_count": Int(reader.relayCount)]` — the EXACT
        // dict the JSON `projections["settings_hub"]` yields (byte-identical, no
        // fabrication). Consumed typed-first in `KernelModel.apply`
        // (`typedSettingsHub ?? update.projections?.settingsHub`); the existing
        // `SettingsHubSummary(relayCount:)` wrap (KernelBridge) stays untouched.
        typed_sidecar: Some(TypedSidecar {
            swift_reader_type: Some("nmp_kernel_SettingsHubSnapshot"),
        }),
    },
    // V-107 / ADR-0070 / ADR-0072-amended: Marmot (MLS-over-Nostr) push
    // projections. Both are registered by `nmp_marmot::install` during
    // explicit Rust composition. The runtime emits empty objects when no
    // local-key Marmot projection is active.
    //
    // `nmp.marmot.snapshot` has no `_` segment, so post-convertFromSnakeCase
    // the key is identical: `"nmp.marmot.snapshot"`. The CodingKeys case
    // still needs an explicit raw value because it is a dotted key that the
    // synthesised decoder (which would pick the property name) cannot match
    // (the generated CodingKeys enum covers ALL cases, not just dotted ones).
    SnapshotProjectionEntry {
        key: "nmp.marmot.snapshot",
        swift_field: "marmotSnapshot",
        swift_type: "MarmotSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // Marmot push-projection batch: the `flatc --swift` reader
            // (`nmp_marmot_MarmotSnapshot`, wrapping `nmp_marmot_MarmotGroupRow` /
            // `nmp_marmot_PendingWelcomeRow` / `nmp_marmot_KeyPackageStatus`) ships
            // with this batch from `crates/nmp-marmot/schema/marmot_snapshot.fbs`.
            // Host-registered typed producer in `crates/nmp-marmot/src/runtime.rs`
            // (`nmp_marmot::install` -> `crate::wire::snapshot_fb::typed_projection`).
            // Nested-vector copy of
            // `groups`/`pendingWelcomes` plus the `keyPackage` sub-table; every
            // `has_*` companion bool maps the optional `String?`/`UInt32?`/`UInt64?`
            // (nil when absent) so the typed value is byte-identical to the JSON
            // path's `null`. The wire's `orphanedCommitCount` diagnostic is NOT
            // carried by the Chirp `MarmotSnapshot` domain type; #1651 the
            // `initErrorKind`/`initErrorDetail` service-init diagnostic (which
            // replaced the former `keyringUnavailable` bool) IS now carried.
            // Consumed by `MarmotStore.apply` via the `KernelModel.swift` fan-out
            // (`result.typedMarmotSnapshot ?? update.projections?.marmotSnapshot`).
            // See `TypedProjectionGlue.marmotSnapshot`.
            swift_reader_type: Some("nmp_marmot_MarmotSnapshot"),
        }),
    },
    // `nmp.marmot.messages` projects a JSON object keyed by `group_id_hex`
    // → newest-N `MarmotMessageRow` array (all groups in one map).
    // Post-convertFromSnakeCase the key is `"nmp.marmot.messages"` (no `_`).
    SnapshotProjectionEntry {
        key: "nmp.marmot.messages",
        swift_field: "marmotMessages",
        swift_type: "[String: [MarmotMessage]]",
        typed_sidecar: Some(TypedSidecar {
            // Marmot push-projection batch: the `flatc --swift` reader
            // (`nmp_marmot_MarmotMessages`, wrapping `nmp_marmot_MarmotGroupMessages`
            // / `nmp_marmot_MarmotMessageRow`) ships with this batch from
            // `crates/nmp-marmot/schema/marmot_messages.fbs`. Host-registered typed
            // producer in `crates/nmp-marmot/src/runtime.rs`
            // (`nmp_marmot::install` -> `crate::wire::messages_fb::typed_projection`).
            // FlatBuffers has no map
            // type, so the producer flattens the `group_id_hex -> [MarmotMessageRow]`
            // JSON map to a `group_id_hex`-sorted `[MarmotGroupMessages]` vector;
            // the glue rebuilds the domain `[String: [MarmotMessage]]` dict
            // (mirroring the `claimed_profiles`/`zaps` flattened-map precedent).
            // `epoch` carries a `has_epoch` companion → `UInt64?` (nil when absent).
            // Consumed by `MarmotStore.apply` via the `KernelModel.swift` fan-out
            // (`result.typedMarmotMessages ?? update.projections?.marmotMessages`).
            // See `TypedProjectionGlue.marmotMessages`.
            swift_reader_type: Some("nmp_marmot_MarmotMessages"),
        }),
    },
];
