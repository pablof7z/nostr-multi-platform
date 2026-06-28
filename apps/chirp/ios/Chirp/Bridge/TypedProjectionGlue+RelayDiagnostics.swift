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
            role: row.role ?? "",
            connection: row.connection ?? "",
            auth: row.auth ?? "",
            totalSubCount: row.totalSubCount,
            activeSubCount: row.activeSubCount,
            eosedSubCount: row.eosedSubCount,
            totalEventsRx: row.totalEventsRx,
            reconnectCount: row.reconnectCount,
            bytesRx: row.bytesRx,
            bytesTx: row.bytesTx,
            lastConnectedMs: row.lastConnectedMs,
            lastEventMs: row.lastEventMs,
            lastNotice: row.hasLastNotice ? (row.lastNotice ?? "") : nil,
            noticeCount: row.noticeCount,
            notices: row.notices.map(relayDiagnosticsNotice),
            lastError: row.hasLastError ? (row.lastError ?? "") : nil,
            wireSubs: row.wireSubs.map(relayDiagnosticsWireSub),
            info: row.info.map(relayDiagnosticsInfo),
            reasons: row.reasons.map(relayConnectionReason),
            discoveryKinds: row.discoveryKinds.map { $0 }
        )
    }
    // `relayDiagnosticsInfo` lives in TypedProjectionGlue+RelayDiagnosticsInfo.swift
    // (pre-existing extraction) — do not redeclare it here.

    private static func relayDiagnosticsNotice(
        _ notice: nmp_kernel_RelayDiagnosticsNotice
    ) -> RelayDiagnosticsNotice {
        RelayDiagnosticsNotice(
            atMs: notice.atMs,
            text: notice.text ?? ""
        )
    }

    private static func relayConnectionReason(
        _ reason: nmp_kernel_RelayConnectionReason
    ) -> RelayConnectionReason {
        var kinds: [UInt32] = []
        let kindsVec = reason.kinds
        for i in 0..<kindsVec.count {
            kinds.append(kindsVec[i])
        }
        return RelayConnectionReason(
            kind: reason.kind ?? "",
            authorPubkeys: reason.authorPubkeys.map { $0 ?? "" },
            authorTotal: reason.authorTotal,
            kinds: kinds,
            sourceEventId: reason.hasSourceEventId ? reason.sourceEventId : nil
        )
    }

    private static func relayDiagnosticsWireSub(
        _ sub: nmp_kernel_RelayDiagnosticsWireSub
    ) -> RelayDiagnosticsWireSub {
        RelayDiagnosticsWireSub(
            wireId: sub.wireId ?? "",
            relayUrl: sub.relayUrl ?? "",
            filterSummary: sub.filterSummary ?? "",
            state: sub.state ?? "",
            consumerCount: sub.consumerCount,
            eventsRx: sub.eventsRx,
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
            refcount: interest.refcount,
            cacheCoverage: interest.cacheCoverage ?? "",
            relayUrls: interest.relayUrls.map { $0 ?? "" }
        )
    }
}
