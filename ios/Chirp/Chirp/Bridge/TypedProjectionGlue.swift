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
}
