import SwiftUI

// Routing-provenance reasons for a relay, plus the Block / Unblock affordance.
//
// Extracted from `RelayDetailView` (Phase 5 — relay attribution). THIN SHELL —
// the Rust projection supplies machine tokens (`kind`, raw `authorTotal`,
// raw `kinds`); this view derives `displayLabel` and `kindsDisplayLabel` from
// them (aim.md §4.5). It derives its own hue from the raw `kind` via
// `DiagnosticsTone.reason` and renders chip lists; it never switches on
// protocol semantics for business logic.
//
// Author pubkeys arrive as hex strings (raw data, ADR-0032). `shortHex`
// abbreviates them for display; the full hex is offered as the copy value
// (bech32 is not available on this projection without a separate encode call).

/// Reasons section + Block / Unblock button for `RelayDetailView`.
///
/// Rendered as a sub-section pair so `RelayDetailView` stays within the
/// 300-line file-size gate (AGENTS.md). Injected via `.environmentObject`.
struct RelayReasonsSection: View {
    let row: RelayDiagnosticsRow
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        if !row.reasons.isEmpty {
            reasonsSection
        }
        blockSection
    }

    // MARK: - Reasons

    private var reasonsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Connection Reasons")
                .font(.headline)
                .foregroundStyle(.primary)
            VStack(spacing: 0) {
                ForEach(Array(row.reasons.enumerated()), id: \.offset) { index, reason in
                    if index > 0 { Divider() }
                    ReasonCard(reason: reason)
                }
            }
            .padding(.horizontal, 12)
        }
    }

    // MARK: - Block / Unblock

    @ViewBuilder
    private var blockSection: some View {
        if row.connectionLabel == "Blocked" {
            Button {
                model.unblockRelay(url: row.relayUrl)
            } label: {
                Text("Unblock Relay")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered)
            .padding(.horizontal, 0)
        } else {
            Button(role: .destructive) {
                model.blockRelay(url: row.relayUrl)
            } label: {
                Text("Block Relay")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered)
        }
    }
}

// MARK: - ReasonCard

/// One routing-provenance card inside the reasons section.
///
/// Renders the shell-computed `displayLabel` (tinted by the shell-derived hue
/// from `kind`), an optional `kindsDisplayLabel`, capped author pubkey chips
/// with an overflow count, and an optional hint-origin event id. NO protocol
/// logic — label/hue mapping is shell-side; this view is purely layout.
private struct ReasonCard: View {
    let reason: RelayConnectionReason

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(reason.displayLabel)
                .font(.callout.weight(.semibold))
                .foregroundStyle(DiagnosticsColor.color(forTone: DiagnosticsTone.reason(reason.kind)))

            if !reason.kindsDisplayLabel.isEmpty {
                Text(reason.kindsDisplayLabel)
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
            }

            if !reason.authorPubkeys.isEmpty {
                authorChipsRow
            }

            if let sourceId = reason.sourceEventId {
                HStack(spacing: 4) {
                    Text("Event:")
                        .font(.caption.weight(.medium))
                        .foregroundStyle(.secondary)
                    Text(sourceId.shortHex)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }
        }
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder
    private var authorChipsRow: some View {
        let shownCount = reason.authorPubkeys.count
        let total = Int(reason.authorTotal)
        let excess = total - shownCount
        VStack(alignment: .leading, spacing: 4) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    ForEach(reason.authorPubkeys, id: \.self) { pubkey in
                        // Raw hex pubkey: display with shortHex abbreviation.
                        // The chip copies the full hex to clipboard; bech32
                        // encoding is not available on this projection without
                        // a separate round-trip (ADR-0032).
                        NostrNpubChip(npub: pubkey, npubShort: pubkey.shortHex)
                    }
                }
            }
            if excess > 0 {
                Text("+\(excess) more (\(total) total)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }
}
