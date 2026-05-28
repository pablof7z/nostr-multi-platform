import SwiftUI

// MARK: - relay-list

/// Renders the relay list component with current relay component rows covering
/// every role/tint token and every connection-status dot variant.
struct RelayListPage: View {

    private var relayRows: [NostrRelayEditRow] {
        GALLERY_SHOWCASE.relays.map { relay in
            NostrRelayEditRow(
                url: relay.url,
                role: relay.role,
                roleLabel: roleLabel(relay.role),
                roleTint: roleTint(relay.role)
            )
        }
    }

    private var statusesByRelay: [String: String] {
        Dictionary(
            uniqueKeysWithValues: GALLERY_SHOWCASE.relays.enumerated().map { index, relay in
                (relay.url, index == 0 ? "connecting" : "connected")
            }
        )
    }

    var body: some View {
        VStack(spacing: 16) {
            sectionCard(caption: "NostrRelayList(relays:connectionStatus:)") {
                NostrRelayList(
                    relays: relayRows,
                    connectionStatus: statusesByRelay
                )
            }
            sectionCard(caption: "Empty state") {
                NostrRelayList(relays: [], connectionStatus: [:])
            }
        }
    }

    @ViewBuilder
    private func sectionCard(caption: String, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(caption)
                .font(.caption)
                .foregroundStyle(.secondary)
            VStack {
                content()
            }
            .frame(maxWidth: .infinity)
            .background(Color(.secondarySystemGroupedBackground))
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
    }

    private func roleLabel(_ role: String) -> String {
        if role.contains("both") && role.contains("indexer") {
            return "Both + Indexer"
        }
        if role.contains("indexer") {
            return "Indexer"
        }
        if role.contains("both") {
            return "Both"
        }
        return role.capitalized
    }

    private func roleTint(_ role: String) -> String {
        if role.contains("both") {
            return "success"
        }
        if role.contains("indexer") {
            return "info"
        }
        return "accent"
    }
}
