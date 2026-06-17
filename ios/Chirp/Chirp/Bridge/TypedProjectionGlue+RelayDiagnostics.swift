import FlatBuffers
import Foundation

// Relay-diagnostics typed-projection glue extracted from TypedProjectionGlue.swift
// to satisfy the 500-LOC file-size hard-cap gate (AGENTS.md). Keeps the
// `enum TypedProjectionGlue` extension pattern used throughout the codebase.

extension TypedProjectionGlue {

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
            lastConnectedMs: row.lastConnectedMs,
            lastEventMs: row.lastEventMs,
            lastNotice: row.hasLastNotice ? (row.lastNotice ?? "") : nil,
            lastError: row.hasLastError ? (row.lastError ?? "") : nil,
            wireSubs: row.wireSubs.map(relayDiagnosticsWireSub),
            info: row.info.map(relayDiagnosticsInfo),
            reasons: row.reasons.map(relayConnectionReason)
        )
    }

    private static func relayDiagnosticsInfo(
        _ info: nmp_kernel_RelayDiagnosticsInfo
    ) -> RelayDiagnosticsInfo {
        RelayDiagnosticsInfo(
            name: info.hasName ? (info.name ?? "") : nil,
            description: info.hasDescription ? (info.description ?? "") : nil,
            icon: info.hasIcon ? (info.icon ?? "") : nil,
            pubkey: info.hasPubkey ? (info.pubkey ?? "") : nil,
            contact: info.hasContact ? (info.contact ?? "") : nil,
            software: info.hasSoftware ? (info.software ?? "") : nil,
            version: info.hasVersion ? (info.version ?? "") : nil,
            supportedNips: info.supportedNips.map { $0 },
            paymentRequired: info.hasPaymentRequired ? info.paymentRequired : nil,
            authRequired: info.hasAuthRequired ? info.authRequired : nil,
            restrictedWrites: info.hasRestrictedWrites ? info.restrictedWrites : nil
        )
    }

    private static func relayConnectionReason(
        _ reason: nmp_kernel_RelayConnectionReason
    ) -> RelayConnectionReason {
        RelayConnectionReason(
            kind: reason.kind ?? "",
            label: reason.label ?? "",
            tone: reason.tone ?? "",
            authorPubkeys: reason.authorPubkeys.map { $0 ?? "" },
            authorTotal: reason.authorTotal,
            kindsLabel: reason.kindsLabel ?? "",
            sourceEventId: reason.hasSourceEventId ? reason.sourceEventId : nil
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
            openedMs: sub.openedMs,
            lastEventMs: sub.lastEventMs,
            eoseMs: sub.eoseMs,
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
}
