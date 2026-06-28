import FlatBuffers
import Foundation

/// HAND-WRITTEN glue between the `flatc --swift` FlatBuffers reader structs and
/// the Chirp domain types, for the typed-projection-sidecar decode path.
///
/// ## Why this is hand-written, not generated
///
/// The generated `TypedProjectionDecoders.generated.swift` owns the mechanical
/// half of every typed-sidecar decoder: the `key`+`schemaId` envelope lookup
/// and the unchecked `getRoot(byteBuffer:)` decode into the reader struct. The reader
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
    /// three-field rows (`value`, `tint`, `isDefault`), in the producer's
    /// picker render order.
    ///
    /// `label` was removed from the wire (#1678, D7); `RelayRoleOption.label`
    /// is now a computed property that maps `value` → English label in the shell.
    static func relayRoleOptions(_ reader: nmp_kernel_RelayRoleOptionsSnapshot) -> [RelayRoleOption] {
        reader.options.map { row in
            RelayRoleOption(
                isDefault: row.isDefault,
                tint: row.tint ?? "",
                value: row.value ?? ""
            )
        }
    }

    // MARK: outbox_summary → OutboxSummary

    /// Map the typed `outbox_summary` sidecar (`KOXS` /
    /// `nmp_kernel_OutboxSummarySnapshot`) to the `OutboxSummary` the JSON
    /// `projections.outbox_summary` path yields. Single-table field-for-field
    /// copy of the raw per-status counters. ADR-0032 / aim.md §2 #4:
    /// `title` / `subtitle` removed from the wire; the shell computes them.
    static func outboxSummary(_ reader: nmp_kernel_OutboxSummarySnapshot) -> OutboxSummary {
        OutboxSummary(
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
    /// ADR-0032 / aim.md §2 #4: `title`, `preview`, `statusLabel`, `systemImage`
    /// removed from the wire; the shell computes them (see helpers in
    /// `NotificationsView+OutboxRow.swift`).
    static func publishOutbox(_ reader: nmp_kernel_PublishOutboxSnapshot) -> [PublishOutboxItem] {
        reader.items.map { item in
            // ADR-0032 / V-115: `createdAtDisplay`/`targetSummary` deprecated;
            // use `createdAt` (raw uint64 Unix seconds) instead.
            PublishOutboxItem(
                handle: item.handle ?? "",
                eventId: item.eventId ?? "",
                kind: item.kind,
                content: item.content ?? "",
                createdAt: item.createdAt,
                status: item.status ?? "",
                canRetry: item.canRetry,
                targetRelays: Int(item.targetRelays),
                relays: item.relays.map { relay in
                    PublishOutboxRelay(
                        relayUrl: relay.relayUrl ?? "",
                        status: relay.status ?? "",
                        attempt: relay.attempt,
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
    /// `targetRelays`, `status` (the wire's `canRetry` / `relayOutcomes`
    /// fields are not decoded by the JSON path either, so ignoring them is
    /// parity-preserving). `targetRelays` widens the wire `uint` to the
    /// domain's `Int`.
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
        case "failed":
            // #1735: lift the curated reason_code (+ subject) when present; an
            // un-coded failure has hasReasonCode == false and degrades to prose.
            return .failed(
                reason: row.hasReason ? (row.reason ?? "") : "",
                reasonCode: row.hasReasonCode ? row.reasonCode : nil,
                reasonSubject: row.hasReasonSubject ? row.reasonSubject : nil)
        case "cancelled": return .cancelled
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
    // Moved to TypedProjectionGlue+RelayDiagnostics.swift (file-size gate).

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

    // MARK: nmp.nip29.group_events → GroupEventsSnapshot

    /// Map the typed `nmp.nip29.group_events` sidecar (`NGEV` /
    /// `nmp_nip29_GroupEventsSnapshot`) to the `GroupEventsSnapshot` the JSON
    /// `projections["nmp.nip29.group_events"]` path yields. Flat field-for-field
    /// copy: one ordered `[GroupEvent]` vector (newest-first; the Rust
    /// projection owns the order, Swift does not re-sort), each row carrying raw
    /// protocol values (`id`/`pubkey` hex, verbatim `content`, Unix-second
    /// `createdAt`, raw `kind`).
    static func groupEvents(_ reader: nmp_nip29_GroupEventsSnapshot) -> GroupEventsSnapshot {
        GroupEventsSnapshot(
            events: reader.events.map { row in
                GroupEvent(
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
    /// the JSON path's `null`. Presentation fields (`displayName`/`initials`/
    /// `subtitle`) are computed by the `DiscoveredGroup` extension (ADR-0032).
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
                    // NIP-29 subgroups (#2319): `parent` is a bare string
                    // (absent → nil == root); `children` is a vector of
                    // strings (absent → empty).
                    parent: row.parent,
                    children: row.children.map { $0 ?? "" }
                )
            }
        )
    }

    // MARK: profile cluster → ProfileCard

    /// Map the SHARED `nmp_kernel_ProfileCard` reader (`ProfileCard.generated.swift`,
    /// `include`d by `profile` / `refs.profile`) to the
    /// Chirp `ProfileCard` domain type — the SAME value the JSON `payload` path
    /// yields. The three `has_*` companion bools reproduce the JSON
    /// `null`-when-`None` semantics (ADR-0032): when `has_x == false` the
    /// optional field is `nil`, regardless of the (empty) string slot.
    private static func profileCard(_ card: nmp_kernel_ProfileCard) -> ProfileCard {
        // V-115 / ADR-0032: `npub` slot fully removed from profile_card.fbs.
        // `ProfileCard` carries only hex `pubkey`; shells encode bech32 themselves.
        ProfileCard(
            pubkey: card.pubkey ?? "",
            displayName: card.hasDisplayName ? (card.displayName ?? "") : nil,
            name: card.hasName ? (card.name ?? "") : nil,
            rawDisplayName: card.hasRawDisplayName ? (card.rawDisplayName ?? "") : nil,
            displayNameCamel: card.hasDisplayNameCamel ? (card.displayNameCamel ?? "") : nil,
            pictureUrl: card.hasPictureUrl ? (card.pictureUrl ?? "") : nil,
            banner: card.hasBanner ? (card.banner ?? "") : nil,
            website: card.hasWebsite ? (card.website ?? "") : nil,
            nip05: card.nip05 ?? "",
            about: card.about ?? "",
            lud16: card.hasLud16 ? (card.lud16 ?? "") : nil,
            lud06: card.hasLud06 ? (card.lud06 ?? "") : nil,
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

    // ADR-0063 Lane H: claimedProfiles() (KCPR) and resolvedProfiles() (KRPR)
    // glue functions deleted. Profile data is now served via the refs.profile
    // KPRF NRRD row-delta sidecar, not these whole-map snapshot projections.

    // MARK: nmp.nip17.dm_inbox → DmInboxSnapshot

    /// Map the typed `nmp.nip17.dm_inbox` sidecar (`NDMI` /
    /// `nmp_nip17_DmInboxSnapshot`) to the JSON-path `DmInboxSnapshot`. Nested-vector copy:
    /// `conversations` → `[DmConversation]`, each carrying its `messages` →
    /// `[DmMessage]`. The Rust projection owns ALL ordering (conversations
    /// newest-thread-first, messages oldest-first) and the `isOutgoing` /
    /// `decryptState` classification — the shell re-sorts NOTHING (thin-shell
    /// rule). `replyTo` is wire `Option<String>` (`has_reply_to` bool) → `nil`
    /// when absent, byte-identical to the JSON path's `null`. `sourceRelays`
    /// maps the always-present vector to `[String]?` (empty buffer → `[]`; the
    /// optional exists only for the older-build JSON decode).
    static func dmInbox(_ reader: nmp_nip17_DmInboxSnapshot) -> DmInboxSnapshot {
        DmInboxSnapshot(
            conversations: reader.conversations.map { convo in
                DmConversation(
                    peerPubkey: convo.peerPubkey ?? "",
                    messages: convo.messages.map { msg in
                        DmMessage(
                            id: msg.id ?? "",
                            senderPubkey: msg.senderPubkey ?? "",
                            content: msg.content ?? "",
                            createdAt: msg.createdAt,
                            replyTo: msg.hasReplyTo ? (msg.replyTo ?? "") : nil,
                            isOutgoing: msg.isOutgoing,
                            sourceRelays: msg.sourceRelays.map { $0 ?? "" }
                        )
                    }
                )
            },
            // §D7 — empty/absent `decryptState` maps to "unavailable" (safe
            // "hide the screen" default, never "ok"), matching the Rust decoder.
            decryptState: (reader.decryptState?.isEmpty == false) ? reader.decryptState! : "unavailable",
            undecryptedCount: reader.undecryptedCount
        )
    }

    // MARK: nmp.nip17.dm_relay_list → DmRelayListSnapshot

    /// Map the typed `nmp.nip17.dm_relay_list` sidecar (`NDRL` /
    /// `nmp_nip17_DmRelayListSnapshot`) to the `DmRelayListSnapshot` the JSON
    /// `projections["nmp.nip17.dm_relay_list"]` path yields. Flat field-for-field
    /// copy: `activePubkey` is an `Option<String>` on the wire
    /// (`has_active_pubkey` companion bool) — preserved as `nil` when absent so
    /// the typed value is byte-identical to the JSON path's `null`. `readRelayUrls`
    /// is the kind:10050 read-eligible relay set, order preserved verbatim (Rust
    /// owns it). This key currently has NO Swift read consumer — the glue exists
    /// for parity so the registry-declared seam is complete and the decoder is
    /// unit-tested.
    static func dmRelayList(_ reader: nmp_nip17_DmRelayListSnapshot) -> DmRelayListSnapshot {
        DmRelayListSnapshot(
            activePubkey: reader.hasActivePubkey ? (reader.activePubkey ?? "") : nil,
            readRelayUrls: reader.readRelayUrls.map { $0 ?? "" }
        )
    }

    // MARK: refs.event row → ClaimedEventDto (ADR-0063 Lane C, #1671)
    //
    // Moved to `TypedProjectionGlue+Refs.swift` (codex NIT: keep this
    // hand-authored file under its file-size cap). See `refRowEvent(_:)` there.

    // MARK: bunker_handshake → BunkerHandshake

    /// Map the typed `bunker_handshake` sidecar (`KBHS` /
    /// `nmp_kernel_BunkerHandshake`) to the `BunkerHandshake` the JSON
    /// `projections["bunker_handshake"]` path yields. `message` is the only
    /// `Option<String>` (its `has_message` companion → `nil` when absent, byte-
    /// identical to the JSON `null`). The five flag bools are always emitted by a
    /// current kernel; the domain type declares them `Bool?` purely for
    /// legacy-kernel forward-compat. `stage` is a non-optional domain `String`
    /// (wire `?? ""`); the English `stageLabel` is shell-derived from it
    /// (#1493 P9 — no longer on the wire).
    static func bunkerHandshake(_ reader: nmp_kernel_BunkerHandshake) -> BunkerHandshake {
        BunkerHandshake(
            stage: reader.stage ?? "",
            message: reader.hasMessage ? (reader.message ?? "") : nil,
            isIdle: reader.isIdle,
            isInFlight: reader.isInFlight,
            isFailed: reader.isFailed,
            isTerminalSuccess: reader.isTerminalSuccess,
            canCancel: reader.canCancel
        )
    }

    // MARK: signer_state → SignerState (ADR-0048 D6, generalises V-14 / #963)

    /// Map the typed `signer_state` sidecar to `SignerState`. Rust pre-computes
    /// every flag; Swift renders verbatim. `reason` uses the `has_reason`
    /// companion. #1493 P9 (labels-to-shells): the display label / tone are NOT
    /// on the wire — `SignerState.statusLabel` / `.statusTone` derive them from
    /// the raw `state` token via `SignerStateTone`.
    static func signerState(_ reader: nmp_kernel_SignerState) -> SignerState {
        SignerState(
            signerKind: reader.signerKind ?? "",
            state: reader.state ?? "",
            reason: reader.hasReason ? (reader.reason ?? "") : nil,
            isReady: reader.isReady,
            isAwaitingApproval: reader.isAwaitingApproval,
            isReconnecting: reader.isReconnecting,
            isUnavailable: reader.isUnavailable,
            isFailed: reader.isFailed
        )
    }

    // MARK: nip46_onboarding → Nip46Onboarding

    /// Map the typed `nip46_onboarding` sidecar (`KN46` /
    /// `nmp_kernel_Nip46Onboarding`) to the `Nip46Onboarding` the JSON
    /// `projections["nip46_onboarding"]` path yields. `signerApps` is the always-
    /// present, never-empty static probe table (nested-vector copy, order
    /// preserved verbatim — Rust owns it). Two `Option<_>` fields carry `has_*`
    /// companions: `stageKind` (the snake_case wire token re-typed to the SAME
    /// `StageKind` enum the JSON path decodes, `.unknown` forward-compat fallback
    /// for any token the host hasn't been re-typed against) and `progressMessage`
    /// (`nil` when absent, byte-identical to JSON `null`). The four flag bools are
    /// non-optional both sides.
    static func nip46Onboarding(_ reader: nmp_kernel_Nip46Onboarding) -> Nip46Onboarding {
        Nip46Onboarding(
            signerApps: reader.signerApps.map { app in
                Nip46Onboarding.SignerApp(
                    scheme: app.scheme ?? "",
                    signerKind: app.signerKind ?? ""
                )
            },
            stageKind: reader.hasStageKind
                ? (Nip46Onboarding.StageKind(rawValue: reader.stageKind ?? "") ?? .unknown)
                : nil,
            progressCode: reader.hasProgressCode ? (reader.progressCode ?? "") : nil,
            progressMessage: reader.hasProgressMessage ? (reader.progressMessage ?? "") : nil,
            isInFlight: reader.isInFlight,
            isFailed: reader.isFailed,
            isTerminalSuccess: reader.isTerminalSuccess,
            canCancel: reader.canCancel
        )
    }

    // MARK: SnapshotFrame envelope (ADR-0044 Tier-3) → TypedSnapshotEnvelope

    /// Map the ADR-0044 typed `SnapshotFrame` envelope fields (read directly off
    /// the `SnapshotFrame` table, NOT a `typed_projections` sidecar) into the
    /// Chirp domain `TypedSnapshotEnvelope`. The producer
    /// (`encode_snapshot_with_envelope`, `kernel/update.rs`) writes ALL envelope
    /// fields as a unit whenever `metrics` is present, so the caller gates the
    /// whole struct on `frame.snapshot.metrics != nil` (the only field whose
    /// FlatBuffers accessor reports presence) and never builds a partial value.
    ///
    /// Every value is a raw mirror of the top-level `KernelSnapshot` fields
    /// (ADR-0032 — hex pubkeys, ms/unix-seconds, raw counts), field-identical to
    /// what the generic JSON `payload` path yields. The `usize`-origin counters
    /// (`storedEvents`, `tombstones`, the `visible*` set, `inserted`/`updated`/
    /// `removed`, `contactsAuthors`, `timelineAuthors`, `estimatedStoreBytes`,
    /// `payloadBytes`) are `UInt64` on the wire and `Int` on the domain type, so
    /// they narrow with `Int(...)` exactly as the JSON `Int` decode does. The
    /// `Option<u128>` ms fields are native-optional both sides (FlatBuffers
    /// `= null` → Swift `UInt64?`), matching the JSON `null`-when-absent decode.
    static func snapshotEnvelope(
        rev: UInt64,
        running: Bool,
        metrics reader: nmp_transport_Metrics,
        relayStatuses: FlatbufferVector<nmp_transport_RelayStatus>,
        logicalInterests: FlatbufferVector<nmp_transport_LogicalInterestStatus>,
        wireSubscriptions: FlatbufferVector<nmp_transport_WireSubscriptionStatus>,
        logs: FlatbufferVector<String?>,
        lastErrorToast: String?,
        lastErrorCategory: String?
    ) -> TypedSnapshotEnvelope {
        TypedSnapshotEnvelope(
            rev: rev,
            running: running,
            metrics: snapshotMetrics(reader),
            relayStatuses: relayStatuses.map(snapshotRelayStatus),
            logicalInterests: logicalInterests.map(snapshotLogicalInterest),
            wireSubscriptions: wireSubscriptions.map(snapshotWireSubscription),
            logs: logs.map { $0 ?? "" },
            lastErrorToast: lastErrorToast,
            lastErrorCategory: lastErrorCategory
        )
    }

    /// Map the typed `Metrics` reader to the `KernelMetrics` domain type
    /// field-for-field. The full-struct mapping is unit-tested for `Equatable`
    /// parity against the JSON decode so a silent field-swap cannot slip through.
    private static func snapshotMetrics(_ m: nmp_transport_Metrics) -> KernelMetrics {
        KernelMetrics(
            actorQueueDepth: m.actorQueueDepth,
            bytesRx: m.bytesRx,
            bytesTx: m.bytesTx,
            claimDropsTotal: m.claimDropsTotal,
            closedRx: m.closedRx,
            contactsAuthors: Int(m.contactsAuthors),
            deleteEvents: m.deleteEvents,
            diagnosticFirehoseEvents: m.diagnosticFirehoseEvents,
            duplicateEvents: m.duplicateEvents,
            emitHzConfigured: m.emitHzConfigured,
            eoseRx: m.eoseRx,
            estimatedStoreBytes: Int(m.estimatedStoreBytes),
            eventsRx: m.eventsRx,
            eventsSinceLastUpdate: m.eventsSinceLastUpdate,
            firstEventMs: m.firstEventMs,
            framesRx: m.framesRx,
            generatedEvents: m.generatedEvents,
            insertedCount: Int(m.insertedCount),
            lastEventToEmitMs: m.lastEventToEmitMs,
            makeUpdateUs: m.makeUpdateUs,
            maxEventToEmitMs: m.maxEventToEmitMs,
            maxEventsPerUpdate: m.maxEventsPerUpdate,
            noteEvents: m.noteEvents,
            noticesRx: m.noticesRx,
            openViews: m.openViews,
            payloadBytes: Int(m.payloadBytes),
            profileEvents: m.profileEvents,
            removedCount: Int(m.removedCount),
            serializeUs: m.serializeUs,
            storeToPayloadRatio: m.storeToPayloadRatio,
            storedEvents: Int(m.storedEvents),
            targetProfileLoadedMs: m.targetProfileLoadedMs,
            timelineAuthors: Int(m.timelineAuthors),
            timelineFirstItemMs: m.timelineFirstItemMs,
            timelineOpenedMs: m.timelineOpenedMs,
            tombstones: Int(m.tombstones),
            updateEmittedMs: m.updateEmittedMs,
            updateFrameDegradationsTotal: m.updateFrameDegradationsTotal,
            updateSequence: m.updateSequence,
            updatedCount: Int(m.updatedCount),
            visibleItems: Int(m.visibleItems),
            visiblePlaceholderAvatarItems: Int(m.visiblePlaceholderAvatarItems),
            visibleProfiledItems: Int(m.visibleProfiledItems)
        )
    }

    /// Map one typed `RelayStatus` reader row to the domain type. `Option<String>`
    /// fields (`lastNotice`, `lastError`, `errorCategory`, `lastCloseReason`)
    /// are absent-on-the-wire → `nil`; the JSON decode yields `nil` for the same
    /// `null`/omitted keys. The `= null` ms fields are native-optional both sides.
    private static func snapshotRelayStatus(_ r: nmp_transport_RelayStatus) -> RelayStatus {
        RelayStatus(
            activeWireSubscriptions: Int(r.activeWireSubscriptions),
            auth: r.auth ?? "",
            bytesRx: r.bytesRx,
            bytesTx: r.bytesTx,
            connection: r.connection ?? "",
            denied: r.denied,
            errorCategory: r.errorCategory,
            eventsRx: r.eventsRx,
            lastCloseReason: r.lastCloseReason,
            lastConnectedAtMs: r.lastConnectedAtMs,
            lastError: r.lastError,
            lastEventAtMs: r.lastEventAtMs,
            lastNotice: r.lastNotice,
            negentropyProbe: r.negentropyProbe ?? "",
            // The transport `RelayStatus` wire table does not carry the per-relay
            // NOTICE counter (it lives on the `Metrics` table + the richer
            // `relay_diagnostics` projection, which is what the diagnostics badge
            // reads). The transport snapshot path therefore reports 0 here.
            noticesRx: 0,
            reconnectCount: r.reconnectCount,
            relayUrl: r.relayUrl ?? "",
            role: r.role ?? ""
        )
    }

    /// Map one typed `LogicalInterestStatus` reader row to the domain type.
    /// `relayUrls` maps the always-present wire vector to `[String]`; the `= null`
    /// `warmingUntilMs` is native-optional both sides.
    private static func snapshotLogicalInterest(
        _ l: nmp_transport_LogicalInterestStatus
    ) -> LogicalInterestStatus {
        LogicalInterestStatus(
            cacheCoverage: l.cacheCoverage ?? "",
            key: l.key ?? "",
            refcount: l.refcount,
            relayUrls: l.relayUrls.map { $0 ?? "" },
            state: l.state ?? "",
            warmingUntilMs: l.warmingUntilMs
        )
    }

    /// Map one typed `WireSubscriptionStatus` reader row to the domain type.
    /// `Option<String>` `closeReason` is absent-on-the-wire → `nil`; the `= null`
    /// ms fields are native-optional both sides.
    private static func snapshotWireSubscription(
        _ w: nmp_transport_WireSubscriptionStatus
    ) -> WireSubscriptionStatus {
        WireSubscriptionStatus(
            closeReason: w.closeReason,
            eoseAtMs: w.eoseAtMs,
            eventsRx: w.eventsRx,
            filterSummary: w.filterSummary ?? "",
            lastEventAtMs: w.lastEventAtMs,
            logicalConsumerCount: w.logicalConsumerCount,
            openedAtMs: w.openedAtMs,
            relayUrl: w.relayUrl ?? "",
            state: w.state ?? "",
            wireId: w.wireId ?? ""
        )
    }

    // MARK: nmp.marmot.snapshot → MarmotSnapshot

    /// Map the typed `nmp.marmot.snapshot` sidecar (`NMMS` /
    /// `nmp_marmot_MarmotSnapshot`) to the `MarmotSnapshot` the JSON
    /// `projections["nmp.marmot.snapshot"]` path yields (V-107 / ADR-0039).
    /// Nested-vector copy: `groups` → `[MarmotGroup]`, `pendingWelcomes` →
    /// `[MarmotPendingWelcome]`, plus the `keyPackage` sub-table → `MarmotKeyPackage`.
    /// The Rust projection sends RAW key-package state (`published`/`ageSecs`/
    /// `stale`/`isRegistered`); the shell derives the subtitle / action-label /
    /// age string itself (aim.md §2 — presentation formatting lives in the shell).
    /// Presentation fallbacks (`displayName`/`initials` for groups,
    /// `displayName` for pending-welcomes, `invitesChipLabel` and
    /// `displayLabel`) are now shell-computed from raw wire counts/names
    /// (schema v4 — aim.md §2). Every `has_*` companion bool reproduces the
    /// JSON `null`-when-`None` semantics: `unreadCount`/`lastMsgAt`
    /// (`UInt32?`/`UInt64?`), `dTag`/`ageSecs` (`String?`/`UInt64?`),
    /// byte-identical to the JSON path. The wire's `orphanedCommitCount`
    /// diagnostic is NOT carried by the Chirp domain type. #1651: the
    /// `initErrorKind` / `initErrorDetail` service-init diagnostic (replacing
    /// the former `keyringUnavailable` bool the domain type dropped) IS now
    /// carried so GroupsView can render a minimal failure surface. A missing
    /// `keyPackage` sub-table (defensive — the producer always emits it) falls
    /// back to `.empty`, matching the JSON decode of an absent object.
    static func marmotSnapshot(_ reader: nmp_marmot_MarmotSnapshot) -> MarmotSnapshot {
        let keyPackage: MarmotKeyPackage = reader.keyPackage.map { kp in
            MarmotKeyPackage(
                published: kp.published, dTag: kp.hasDTag ? (kp.dTag ?? "") : nil,
                ageSecs: kp.hasAgeSecs ? kp.ageSecs : nil, stale: kp.stale,
                isRegistered: kp.isRegistered
            )
        } ?? .empty
        return MarmotSnapshot(
            groups: reader.groups.map { g in
                MarmotGroup(
                    idHex: g.idHex ?? "",
                    name: g.name ?? "",
                    members: g.members.map { $0 ?? "" },
                    memberCount: g.memberCount,
                    unreadCount: g.hasUnreadCount ? g.unreadCount : nil,
                    lastMsgAt: g.hasLastMsgAt ? g.lastMsgAt : nil
                )
            },
            pendingWelcomes: reader.pendingWelcomes.map {
                MarmotPendingWelcome(idHex: $0.idHex ?? "", groupName: $0.groupName ?? "",
                                     inviterNpub: $0.inviterNpub ?? "")
            },
            keyPackage: keyPackage,
            cachedKpPubkeys: reader.cachedKpPubkeys.map { $0 ?? "" },
            isRegistered: reader.isRegistered,
            pendingOps: reader.pendingOps.map { op in
                MarmotPendingOp(correlationId: op.correlationId ?? "", opTag: op.opTag ?? "",
                                missingCount: op.missingCount, ageSecs: op.ageSecs)
            },
            lastOpError: reader.lastOpError.map { e in
                MarmotLastOpError(op: e.op ?? "", reason: e.reason ?? "",
                                  atSecs: e.atSecs, correlationId: e.correlationId ?? "")
            },
            // #1651 service-init failure raw tokens ("" = none). The Chirp
            // domain type now carries these (formerly dropped); GroupsView
            // renders a minimal diagnostic from initErrorKind.
            initErrorKind: reader.initErrorKind ?? "",
            initErrorDetail: reader.initErrorDetail ?? ""
        )
    }

    // MARK: nmp.marmot.messages → [String: [MarmotMessage]]

    /// Map the typed `nmp.marmot.messages` sidecar (`NMMG` /
    /// `nmp_marmot_MarmotMessages`) to the `[String: [MarmotMessage]]` the JSON
    /// `projections["nmp.marmot.messages"]` path yields (V-107 / ADR-0039).
    /// FlatBuffers has no map type, so the producer flattens the
    /// `group_id_hex -> [MarmotMessageRow]` map to a `group_id_hex`-sorted
    /// `[MarmotGroupMessages]` vector; this rebuilds the dictionary keyed by
    /// `groupIdHex`, mirroring the `claimed_profiles`/`zaps` flattened-map
    /// precedent. Each group's `messages` order is preserved verbatim (newest-N
    /// rows; Rust owns the order — the shell re-sorts NOTHING). `epoch` carries a
    /// `has_epoch` companion → `UInt64?` (`nil` when absent, byte-identical to the
    /// JSON path's `null`).
    static func marmotMessages(
        _ reader: nmp_marmot_MarmotMessages
    ) -> [String: [MarmotMessage]] {
        reader.groups.reduce(into: [String: [MarmotMessage]]()) { out, group in
            guard let key = group.groupIdHex else { return }
            out[key] = group.messages.map { msg in
                MarmotMessage(
                    id: msg.id ?? "",
                    senderPubkeyHex: msg.senderPubkeyHex ?? "",
                    content: msg.content ?? "",
                    createdAt: msg.createdAt,
                    epoch: msg.hasEpoch ? msg.epoch : nil
                )
            }
        }
    }

    // MARK: wallet → WalletStatusData

    /// Map the typed `wallet` sidecar (`NWST` / `nmp_nip47_WalletStatus`) to the
    /// `WalletStatusData` the JSON `projections["wallet"]` path yields — a
    /// field-SUBSET of the wire (no `walletNpubShort`/`connectionState`).
    /// The `has_*` companion bools reproduce JSON `null`-when-`None`:
    /// `balanceMsats`/`balanceSats` are `nil` when `false`.
    ///
    /// RAW-DATA DOCTRINE (aim.md §2 / ADR-0032): the wire carries only the raw
    /// `status` token. `WalletStatusData.statusLabel` / `.statusTone` derive the
    /// label and tone from it (via `WalletStatusTone`), and the view formats the
    /// balance locally — no presentation strings are read off the buffer.
    static func wallet(_ reader: nmp_nip47_WalletStatus) -> WalletStatusData {
        WalletStatusData(
            status: reader.status ?? "",
            relayUrl: reader.relayUrl ?? "",
            walletPubkeyHex: reader.walletPubkeyHex ?? "",
            walletNpub: reader.walletNpub ?? "",
            balanceMsats: reader.hasBalanceMsats ? reader.balanceMsats : nil,
            balanceSats: reader.hasBalanceSats ? Int(reader.balanceSats) : nil,
            isReady: reader.isReady,
            isConnected: reader.isConnected
        )
    }

    // MARK: settings_hub → [String: Int]

    /// Map the typed `settings_hub` sidecar (`KSHB` /
    /// `nmp_kernel_SettingsHubSnapshot`) to the `[String: Int]` the JSON
    /// `projections["settings_hub"]` path yields. The kernel built-in emits a
    /// single `relay_count:uint`; this rebuilds the SAME one-key dict
    /// (`["relay_count": Int(reader.relayCount)]`) the JSON object decodes to —
    /// byte-identical, no fabrication. The downstream `SettingsHubSummary
    /// (relayCount:)` wrap (`KernelBridge`) reads `dict["relay_count"]`, so this
    /// dict shape preserves every consumer unchanged.
    static func settingsHub(_ reader: nmp_kernel_SettingsHubSnapshot) -> [String: Int] {
        ["relay_count": Int(reader.relayCount)]
    }

    // MARK: action_results → [LastActionResult]

    /// Map the typed `action_results` sidecar (`KARS` /
    /// `nmp_kernel_ActionResultsSnapshot`) to the `[LastActionResult]` the JSON
    /// `projections.action_results` path yields. Per-tick drain array — maps each
    /// `ActionResult` row to `LastActionResult` field-for-field. The two `has_*`
    /// companion bools (`has_error`, `has_result`) preserve the JSON
    /// `null`-when-`None` semantics: `error` is `nil` when `has_error == false`,
    /// byte-identical to the JSON path. `result` is not part of the Chirp
    /// `LastActionResult` domain type (field-subset), so it is ignored here.
    static func actionResults(_ reader: nmp_kernel_ActionResultsSnapshot) -> [LastActionResult] {
        reader.results.map { row in
            LastActionResult(
                correlationId: row.correlationId ?? "",
                status: row.status ?? "",
                error: row.hasError ? (row.error ?? "") : nil
            )
        }
    }

    // MARK: action_stages → [String: [ActionStageEntry]]

    /// Map the typed `action_stages` sidecar (`KAST` /
    /// `nmp_kernel_ActionStagesSnapshot`) to the `[String: [ActionStageEntry]]` the
    /// JSON `projections.action_stages` path yields. The FlatBuffers wire uses a
    /// flat vector of `ActionStagesEntry` rows (one per correlation_id with its own
    /// `stages` vector) instead of a JSON object; this rebuilds the dictionary.
    /// Each stage reconstructs the `ActionStage` enum mirroring the JSON
    /// `init(from:)` switch (snake_case tags; `failed` lifts `has_reason`/`reason`;
    /// unknown tags collapse to `.unknown(raw:)` for D1 forward-compat).
    static func actionStages(
        _ reader: nmp_kernel_ActionStagesSnapshot
    ) -> [String: [ActionStageEntry]] {
        reader.entries.reduce(into: [String: [ActionStageEntry]]()) { out, entry in
            guard let key = entry.key else { return }
            out[key] = entry.stages.map(actionStageEntry)
        }
    }

    private static func actionStageEntry(_ row: nmp_kernel_ActionStageEntry) -> ActionStageEntry {
        let stage: ActionStage
        switch row.stage ?? "" {
        case "requested": stage = .requested
        case "awaiting_capability", "awaitingCapability": stage = .awaitingCapability
        case "publishing": stage = .publishing
        case "accepted": stage = .accepted
        case "failed": stage = .failed(reason: row.hasReason ? (row.reason ?? "") : "")
        case "cancelled": stage = .cancelled
        case let raw: stage = .unknown(raw: raw)
        }
        return ActionStageEntry(stage: stage, atMs: row.atMs)
    }

    // V-112 (ADR-0042): authorView(), profileAction(), timelineItem(),
    // threadView() all deleted — author_view / thread_view typed sidecars
    // (AuthorView.generated.swift, ThreadView.generated.swift) removed from kernel.
}
