import FlatBuffers
import Foundation

/// HAND-WRITTEN glue between the `flatc --swift` FlatBuffers reader structs and
/// the Chirp domain types, for the typed-projection-sidecar decode path.
///
/// ## Why this is hand-written, not generated
///
/// The generated `TypedProjectionDecoders.generated.swift` owns the mechanical
/// half of every typed-sidecar decoder: the `key`+`schemaId` envelope lookup
/// and the `getCheckedRoot(fileId:)` decode into the reader struct. The reader
/// struct's field layout (the FlatBuffer *wire*) does NOT field-align with the
/// Chirp *domain* type — the domain types are field-subsets of the wire, carry
/// `has_*` companion-bool optionals, and (for thick keys) nested sub-buffers.
/// A generic that mapped wire→domain across all keys would be leaky, so that
/// mapping stays here, one static per projection key, matching the
/// `swift_field` the registry assigns.
///
/// Each function takes the generated reader struct and returns the SAME Chirp
/// domain value the generic JSON `payload` path yields for that key, so a
/// consumer can read typed-first and fall back to JSON identically. NOTE: no
/// read site consumes these yet — this is the consumer-side FOUNDATION; wiring
/// the read sites (e.g. `KernelModel`/`KernelBridge`) is the follow-up batch.
/// Raw protocol values only (D11 — no display helpers).
enum TypedProjectionGlue {
    // MARK: accounts → [AccountSummary]

    /// Map the typed `accounts` sidecar (`KACC` / `nmp_kernel_AccountsSnapshot`)
    /// to the `[AccountSummary]` the JSON `projections.accounts` path yields.
    ///
    /// Each `AccountSummaryRow` mirrors the JSON `AccountSummary` field-for-field;
    /// the two `has_*` companion bools (`has_display_name`, `has_picture_url`)
    /// reproduce the JSON `null` / omitted-key semantics (ADR-0032).
    static func accounts(_ reader: nmp_kernel_AccountsSnapshot) -> [AccountSummary] {
        reader.accounts.map { row in
            AccountSummary(
                displayName: row.hasDisplayName ? (row.displayName ?? "") : nil,
                id: row.id ?? "",
                isActive: row.isActive,
                npub: row.npub ?? "",
                pictureUrl: row.hasPictureUrl ? (row.pictureUrl ?? "") : nil,
                signerIsRemote: row.signerIsRemote,
                signerKind: row.signerKind ?? "",
                signerLabel: row.signerLabel ?? "",
                status: row.status ?? ""
            )
        }
    }

    // MARK: active_account → String?

    /// Map the typed `active_account` sidecar (`KACT` /
    /// `nmp_kernel_ActiveAccountSnapshot`) to the `String?` the JSON
    /// `projections.active_account` path yields — `nil` when no account is
    /// active (`has_active_account == false` mirrors JSON `null`).
    static func activeAccount(_ reader: nmp_kernel_ActiveAccountSnapshot) -> String? {
        reader.hasActiveAccount ? (reader.pubkey ?? "") : nil
    }

    // MARK: configured_relays → [AppRelay]

    /// Map the typed `configured_relays` sidecar (`KCRL` /
    /// `nmp_kernel_ConfiguredRelaysSnapshot`) to the `[AppRelay]` the JSON
    /// `projections.configured_relays` path yields. Field-for-field copy of the
    /// two-field `ConfiguredRelay` rows (`url`, canonicalised `role`), in
    /// producer order. No `has_*` companion bools — both strings are always
    /// present (empty when the producer slice carries an empty string).
    static func configuredRelays(_ reader: nmp_kernel_ConfiguredRelaysSnapshot) -> [AppRelay] {
        reader.relays.map { row in
            AppRelay(role: row.role ?? "", url: row.url ?? "")
        }
    }

    // MARK: relay_role_options → [RelayRoleOption]

    /// Map the typed `relay_role_options` sidecar (`KRRO` /
    /// `nmp_kernel_RelayRoleOptionsSnapshot`) to the `[RelayRoleOption]` the JSON
    /// `projections.relay_role_options` path yields. Field-for-field copy of the
    /// four-field rows (`value`, `label`, `tint`, `isDefault`), in the producer's
    /// picker render order.
    static func relayRoleOptions(_ reader: nmp_kernel_RelayRoleOptionsSnapshot) -> [RelayRoleOption] {
        reader.options.map { row in
            RelayRoleOption(
                isDefault: row.isDefault,
                label: row.label ?? "",
                tint: row.tint ?? "",
                value: row.value ?? ""
            )
        }
    }

    // MARK: outbox_summary → OutboxSummary

    /// Map the typed `outbox_summary` sidecar (`KOXS` /
    /// `nmp_kernel_OutboxSummarySnapshot`) to the `OutboxSummary` the JSON
    /// `projections.outbox_summary` path yields. Single-table field-for-field
    /// copy — the kernel owns both the counters AND the pre-formatted
    /// `title` / `subtitle` strings (§6 anti-pattern #1), so the shell binds
    /// them verbatim.
    static func outboxSummary(_ reader: nmp_kernel_OutboxSummarySnapshot) -> OutboxSummary {
        OutboxSummary(
            title: reader.title ?? "",
            subtitle: reader.subtitle ?? "",
            total: reader.total,
            sending: reader.sending,
            retrying: reader.retrying,
            queued: reader.queued,
            failed: reader.failed
        )
    }

    // MARK: publish_outbox → [PublishOutboxItem]

    /// Map the typed `publish_outbox` sidecar (`KPBO` /
    /// `nmp_kernel_PublishOutboxSnapshot`) to the `[PublishOutboxItem]` the JSON
    /// `projections.publish_outbox` path yields. Field-for-field copy of each
    /// in-flight item plus its nested `[PublishOutboxRelay]` rows, in producer
    /// order. `targetRelays` widens the wire `uint` to the domain's `Int`.
    /// `relayReason` is `skip_serializing_if = "String::is_empty"` on the wire —
    /// the JSON path drops the key (decoded as `""`); the buffer carries an empty
    /// string, so both paths yield the same `""` (parity-preserving).
    static func publishOutbox(_ reader: nmp_kernel_PublishOutboxSnapshot) -> [PublishOutboxItem] {
        reader.items.map { item in
            PublishOutboxItem(
                handle: item.handle ?? "",
                eventId: item.eventId ?? "",
                kind: item.kind,
                title: item.title ?? "",
                preview: item.preview ?? "",
                createdAtDisplay: item.createdAtDisplay ?? "",
                status: item.status ?? "",
                statusLabel: item.statusLabel ?? "",
                systemImage: item.systemImage ?? "",
                canRetry: item.canRetry,
                targetRelays: Int(item.targetRelays),
                targetSummary: item.targetSummary ?? "",
                relays: item.relays.map { relay in
                    PublishOutboxRelay(
                        relayUrl: relay.relayUrl ?? "",
                        status: relay.status ?? "",
                        statusLabel: relay.statusLabel ?? "",
                        attempt: relay.attempt,
                        attemptLabel: relay.attemptLabel ?? "",
                        message: relay.message ?? "",
                        relayReason: relay.relayReason ?? ""
                    )
                }
            )
        }
    }

    // MARK: publish_queue → [PublishQueueEntry]

    /// Map the typed `publish_queue` sidecar (`KPBQ` /
    /// `nmp_kernel_PublishQueueSnapshot`) to the `[PublishQueueEntry]` the JSON
    /// `projections.publish_queue` path yields. The Chirp domain type is a
    /// FIELD-SUBSET of the wire — it consumes only `eventId`, `kind`,
    /// `targetRelays`, `status` (the wire's `title` / `canRetry` /
    /// `relayOutcomes` fields are not decoded by the JSON path either, so
    /// ignoring them is parity-preserving). `targetRelays` widens the wire
    /// `uint` to the domain's `Int`.
    static func publishQueue(_ reader: nmp_kernel_PublishQueueSnapshot) -> [PublishQueueEntry] {
        reader.entries.map { entry in
            PublishQueueEntry(
                eventId: entry.eventId ?? "",
                kind: entry.kind,
                targetRelays: Int(entry.targetRelays),
                status: entry.status ?? ""
            )
        }
    }

    // MARK: action_lifecycle → ActionLifecycleSnapshot

    /// Reconstruct the `ActionLifecycleStage` enum from one `flatc --swift`
    /// `LifecycleEntry` reader row. Mirrors the JSON path's `init(from:)` switch
    /// in `ActionLifecycleEntry` (KernelBridge.swift): the closed snake_case
    /// vocabulary maps to the typed cases; `failed` lifts the `reason` sibling
    /// (carried with `has_reason`); any unrecognised wire stage collapses to
    /// `.unknown(raw:)` for forward-compat (D1).
    private static func lifecycleStage(_ row: nmp_kernel_LifecycleEntry) -> ActionLifecycleStage {
        switch row.stage ?? "" {
        case "requested": return .requested
        case "awaiting_capability", "awaitingCapability": return .awaitingCapability
        case "publishing": return .publishing
        case "accepted": return .accepted
        case "failed": return .failed(reason: row.hasReason ? (row.reason ?? "") : "")
        case let raw: return .unknown(raw: raw)
        }
    }

    private static func lifecycleEntry(_ row: nmp_kernel_LifecycleEntry) -> ActionLifecycleEntry {
        ActionLifecycleEntry(
            correlationId: row.correlationId ?? "",
            stage: lifecycleStage(row)
        )
    }

    /// Map the typed `action_lifecycle` sidecar (`KALC` /
    /// `nmp_kernel_ActionLifecycleSnapshot`) to the `ActionLifecycleSnapshot` the
    /// JSON `projections.action_lifecycle` path yields. Two ordered arrays
    /// (`in_flight` / `recent_terminal`); each `LifecycleEntry` row reconstructs
    /// the `ActionLifecycleStage` enum (see `lifecycleStage`). Producer order is
    /// preserved verbatim (parity with the JSON arrays).
    static func actionLifecycle(_ reader: nmp_kernel_ActionLifecycleSnapshot) -> ActionLifecycleSnapshot {
        ActionLifecycleSnapshot(
            inFlight: reader.inFlight.map(lifecycleEntry),
            recentTerminal: reader.recentTerminal.map(lifecycleEntry)
        )
    }

    // MARK: relay_diagnostics → RelayDiagnosticsSnapshot

    /// Map the typed `relay_diagnostics` sidecar (`KRDG` /
    /// `nmp_kernel_RelayDiagnosticsSnapshot`) to the `RelayDiagnosticsSnapshot`
    /// the JSON `projections.relay_diagnostics` path yields. Pure field-for-field
    /// copy of the rolled-up relay rows (each with nested wire-sub rows) plus the
    /// logical-interest rows, in producer order. Every `Option<String>` on the
    /// wire carries a `has_*` companion bool: `has_* == false` maps to the
    /// domain's `nil` (the JSON path's `null`/absent), `true` to the carried
    /// string — so the typed and JSON forms are byte-identical by construction
    /// (the #1031 convention; the kernel captures the produced struct once per
    /// tick so the wall-clock-relative labels never straddle a one-second bucket).
    static func relayDiagnostics(_ reader: nmp_kernel_RelayDiagnosticsSnapshot) -> RelayDiagnosticsSnapshot {
        RelayDiagnosticsSnapshot(
            relays: reader.relays.map(relayDiagnosticsRow),
            interests: reader.interests.map(relayDiagnosticsInterest)
        )
    }

    private static func relayDiagnosticsRow(
        _ row: nmp_kernel_RelayDiagnosticsRow
    ) -> RelayDiagnosticsRow {
        RelayDiagnosticsRow(
            relayUrl: row.relayUrl ?? "",
            shortUrl: row.shortUrl ?? "",
            roleLabel: row.roleLabel ?? "",
            roleTone: row.roleTone ?? "",
            connectionLabel: row.connectionLabel ?? "",
            connectionTone: row.connectionTone ?? "",
            authLabel: row.authLabel ?? "",
            authTone: row.authTone ?? "",
            totalSubCount: row.totalSubCount,
            activeSubCount: row.activeSubCount,
            eosedSubCount: row.eosedSubCount,
            totalEventsRx: row.totalEventsRx,
            totalEventsDisplay: row.totalEventsDisplay ?? "",
            reconnectCount: row.reconnectCount,
            bytesRxDisplay: row.hasBytesRxDisplay ? (row.bytesRxDisplay ?? "") : nil,
            bytesTxDisplay: row.hasBytesTxDisplay ? (row.bytesTxDisplay ?? "") : nil,
            lastConnectedDisplay: row.hasLastConnectedDisplay ? (row.lastConnectedDisplay ?? "") : nil,
            lastEventDisplay: row.hasLastEventDisplay ? (row.lastEventDisplay ?? "") : nil,
            lastNotice: row.hasLastNotice ? (row.lastNotice ?? "") : nil,
            lastError: row.hasLastError ? (row.lastError ?? "") : nil,
            wireSubs: row.wireSubs.map(relayDiagnosticsWireSub)
        )
    }

    private static func relayDiagnosticsWireSub(
        _ sub: nmp_kernel_RelayDiagnosticsWireSub
    ) -> RelayDiagnosticsWireSub {
        RelayDiagnosticsWireSub(
            wireId: sub.wireId ?? "",
            shortWireId: sub.shortWireId ?? "",
            relayUrl: sub.relayUrl ?? "",
            filterSummary: sub.filterSummary ?? "",
            stateLabel: sub.stateLabel ?? "",
            stateTone: sub.stateTone ?? "",
            consumerCountLabel: sub.consumerCountLabel ?? "",
            eventsRxDisplay: sub.hasEventsRxDisplay ? (sub.eventsRxDisplay ?? "") : nil,
            eoseObserved: sub.eoseObserved,
            openedDisplay: sub.openedDisplay ?? "",
            lastEventDisplay: sub.hasLastEventDisplay ? (sub.lastEventDisplay ?? "") : nil,
            eoseDisplay: sub.hasEoseDisplay ? (sub.eoseDisplay ?? "") : nil,
            closeReason: sub.hasCloseReason ? (sub.closeReason ?? "") : nil
        )
    }

    private static func relayDiagnosticsInterest(
        _ interest: nmp_kernel_RelayDiagnosticsInterest
    ) -> RelayDiagnosticsInterest {
        RelayDiagnosticsInterest(
            key: interest.key ?? "",
            state: interest.state ?? "",
            stateTone: interest.stateTone ?? "",
            refcount: interest.refcount,
            cacheCoverage: interest.cacheCoverage ?? "",
            relayUrls: interest.relayUrls.map { $0 ?? "" }
        )
    }

    // MARK: nmp.follow_list → FollowListSnapshot

    /// Map the typed `nmp.follow_list` sidecar (`NF02` /
    /// `nmp_nip02_FollowListSnapshot`) to the `FollowListSnapshot` the JSON
    /// `projections["nmp.follow_list"]` path yields. Flat field-for-field copy:
    /// one ordered `[FollowEntry]` vector, each row a single raw hex `pubkey`
    /// (presentation formatting is a host concern — aim.md §2). Producer order is
    /// preserved verbatim (parity with the JSON array).
    static func followList(_ reader: nmp_nip02_FollowListSnapshot) -> FollowListSnapshot {
        FollowListSnapshot(
            follows: reader.follows.map { FollowEntry(pubkey: $0.pubkey ?? "") }
        )
    }

    // MARK: nmp.nip57.zaps → ZapsAggregateSnapshot

    /// Map the typed `nmp.nip57.zaps` sidecar (`NZAP` / `nmp_nip57_ZapsSnapshot`)
    /// to the `ZapsAggregateSnapshot` the JSON `projections["nmp.nip57.zaps"]`
    /// path yields. FlatBuffers has no map type, so the wire flattens the Rust
    /// `totals: HashMap<EventId, ZapCount>` into a `[ZapTotal]` vector — this
    /// glue rebuilds the dict keyed by `target_event_id` (hex), mirroring the
    /// serde shape. A duplicate target id would collide, but the producer emits
    /// one row per map entry, so keys are unique by construction.
    static func zaps(_ reader: nmp_nip57_ZapsSnapshot) -> ZapsAggregateSnapshot {
        var totals: [String: ZapCount] = [:]
        totals.reserveCapacity(reader.totals.count)
        for row in reader.totals {
            totals[row.targetEventId ?? ""] = ZapCount(
                totalMsats: row.totalMsats,
                count: row.count
            )
        }
        return ZapsAggregateSnapshot(totals: totals)
    }

    // MARK: nmp.nip29.group_chat → GroupChatSnapshot

    /// Map the typed `nmp.nip29.group_chat` sidecar (`NGCS` /
    /// `nmp_nip29_GroupChatSnapshot`) to the `GroupChatSnapshot` the JSON
    /// `projections["nmp.nip29.group_chat"]` path yields. Flat field-for-field
    /// copy: one ordered `[GroupChatMessage]` vector (newest-first; the Rust
    /// projection owns the order, Swift does not re-sort), each row carrying raw
    /// protocol values (`id`/`pubkey` hex, verbatim `content`, Unix-second
    /// `createdAt`, raw `kind`).
    static func groupChat(_ reader: nmp_nip29_GroupChatSnapshot) -> GroupChatSnapshot {
        GroupChatSnapshot(
            messages: reader.messages.map { row in
                GroupChatMessage(
                    id: row.id ?? "",
                    pubkey: row.pubkey ?? "",
                    content: row.content ?? "",
                    createdAt: row.createdAt,
                    kind: row.kind
                )
            }
        )
    }

    // MARK: nmp.nip29.discovered_groups → DiscoveredGroupsSnapshot

    /// Map the typed `nmp.nip29.discovered_groups` sidecar (`NDGS` /
    /// `nmp_nip29_DiscoveredGroupsSnapshot`) to the `DiscoveredGroupsSnapshot`
    /// the JSON `projections["nmp.nip29.discovered_groups"]` path yields. Flat
    /// field-for-field copy: a top-level `hostRelayUrl` plus one ordered
    /// `[DiscoveredGroup]` vector (alphabetical by `groupId`; Rust owns the
    /// order). `name`/`picture`/`about` are tag-derived `Option<String>` on the
    /// wire — bare FlatBuffers strings where absent decodes to `nil`; the glue
    /// preserves that `nil` (NOT `?? ""`) so the typed value is byte-identical to
    /// the JSON path's `null`. The V-24 thin-shell display fields
    /// (`initials`/`displayName`/`subtitle`) travel pre-computed and are copied
    /// verbatim (never re-derived host-side, ADR-0032).
    static func discoveredGroups(
        _ reader: nmp_nip29_DiscoveredGroupsSnapshot
    ) -> DiscoveredGroupsSnapshot {
        DiscoveredGroupsSnapshot(
            hostRelayUrl: reader.hostRelayUrl ?? "",
            groups: reader.groups.map { row in
                DiscoveredGroup(
                    groupId: row.groupId ?? "",
                    hostRelayUrl: row.hostRelayUrl ?? "",
                    name: row.name,
                    picture: row.picture,
                    about: row.about,
                    memberCount: row.memberCount,
                    adminCount: row.adminCount,
                    public: row.public_,
                    open: row.open_,
                    initials: row.initials ?? "",
                    displayName: row.displayName ?? "",
                    subtitle: row.subtitle ?? ""
                )
            }
        )
    }

    // MARK: profile cluster → ProfileCard

    /// Map the SHARED `nmp_kernel_ProfileCard` reader (`ProfileCard.generated.swift`,
    /// `include`d by `profile` / `claimed_profiles` / `resolved_profiles`) to the
    /// Chirp `ProfileCard` domain type — the SAME value the JSON `payload` path
    /// yields. The three `has_*` companion bools reproduce the JSON
    /// `null`-when-`None` semantics (ADR-0032): when `has_x == false` the
    /// optional field is `nil`, regardless of the (empty) string slot.
    private static func profileCard(_ card: nmp_kernel_ProfileCard) -> ProfileCard {
        ProfileCard(
            pubkey: card.pubkey ?? "",
            npub: card.npub ?? "",
            displayName: card.hasDisplayName ? (card.displayName ?? "") : nil,
            pictureUrl: card.hasPictureUrl ? (card.pictureUrl ?? "") : nil,
            nip05: card.nip05 ?? "",
            about: card.about ?? "",
            hasProfile: card.hasProfile,
            lnurl: card.hasLnurl ? (card.lnurl ?? "") : nil
        )
    }

    // MARK: profile → ProfileCard

    /// Map the typed `profile` sidecar (`KPRF` / `nmp_kernel_ProfileSnapshot`) to
    /// the `ProfileCard` the JSON `projections.profile` path yields — the active
    /// account's card.
    static func profile(_ reader: nmp_kernel_ProfileSnapshot) -> ProfileCard? {
        reader.card.map(profileCard)
    }

    // MARK: claimed_profiles → [String: ProfileCard]

    /// Map the typed `claimed_profiles` sidecar (`KCPR` /
    /// `nmp_kernel_ClaimedProfilesSnapshot`) to the `[String: ProfileCard]` the
    /// JSON `projections.claimedProfiles` path yields. FlatBuffers has no map
    /// type, so the producer flattens the `pubkey -> ProfileCard` map to a
    /// key-sorted `[{key, value}]` vector; this rebuilds the dictionary.
    static func claimedProfiles(
        _ reader: nmp_kernel_ClaimedProfilesSnapshot
    ) -> [String: ProfileCard] {
        reader.entries.reduce(into: [String: ProfileCard]()) { out, entry in
            guard let key = entry.key, let value = entry.value else { return }
            out[key] = profileCard(value)
        }
    }

    // MARK: resolved_profiles → [String: ProfileCard]

    /// Map the typed `resolved_profiles` sidecar (`KRPR` /
    /// `nmp_kernel_ResolvedProfilesSnapshot`) to the `[String: ProfileCard]` the
    /// JSON `projections.resolvedProfiles` path yields — the pre-merged
    /// pubkey -> card map (claimed > author_view > mention precedence applied in
    /// Rust). Same flattened-vector shape as `claimed_profiles`.
    static func resolvedProfiles(
        _ reader: nmp_kernel_ResolvedProfilesSnapshot
    ) -> [String: ProfileCard] {
        reader.entries.reduce(into: [String: ProfileCard]()) { out, entry in
            guard let key = entry.key, let value = entry.value else { return }
            out[key] = profileCard(value)
        }
    }
}
