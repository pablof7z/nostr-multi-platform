import SwiftUI

// MARK: - relay-list

/// Renders the relay list component with realistic sample data covering
/// every role/tint token and every connection-status dot variant.
struct RelayListPage: View {

    private let sampleRelays: [NostrRelayEditRow] = [
        NostrRelayEditRow(
            url: "wss://relay.damus.io",
            role: "write",
            roleLabel: "Write",
            roleTint: "accent"),
        NostrRelayEditRow(
            url: "wss://relay.nostr.band",
            role: "read",
            roleLabel: "Read",
            roleTint: "info"),
        NostrRelayEditRow(
            url: "wss://nos.lol",
            role: "both",
            roleLabel: "Both",
            roleTint: "success"),
        NostrRelayEditRow(
            url: "wss://relay.primal.net",
            role: "read",
            roleLabel: "Read",
            roleTint: "info"),
        NostrRelayEditRow(
            url: "wss://purplepag.es",
            role: "write",
            roleLabel: "Write",
            roleTint: "accent"),
    ]

    private let sampleStatuses: [String: String] = [
        "wss://relay.damus.io":    "connected",
        "wss://relay.nostr.band":  "connected",
        "wss://nos.lol":           "connecting",
        "wss://relay.primal.net":  "disconnected",
        "wss://purplepag.es":      "error",
    ]

    var body: some View {
        VStack(spacing: 16) {
            sectionCard(caption: "NostrRelayList(relays:connectionStatus:)") {
                NostrRelayList(
                    relays: sampleRelays,
                    connectionStatus: sampleStatuses
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
}
