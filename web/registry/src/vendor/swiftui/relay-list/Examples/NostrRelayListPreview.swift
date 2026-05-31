import SwiftUI

struct NostrRelayListPreview: View {
    var body: some View {
        NostrRelayList(
            relays: [
                NostrRelayEditRow(
                    url: "wss://relay.damus.io",
                    role: "both"
                ),
                NostrRelayEditRow(
                    url: "wss://nos.lol",
                    role: "read"
                ),
                NostrRelayEditRow(
                    url: "wss://relay.snort.social",
                    role: "write"
                ),
            ],
            relayRoleOptions: [
                RelayRoleOption(value: "both", label: "Both", tint: "accent", isDefault: true),
                RelayRoleOption(value: "read", label: "Read", tint: "info", isDefault: false),
                RelayRoleOption(value: "write", label: "Write", tint: "success", isDefault: false),
            ],
            connectionStatus: [
                "wss://relay.damus.io": "connected",
                "wss://nos.lol": "connecting",
                "wss://relay.snort.social": "disconnected",
            ],
            onRelayTap: { _ in }
        )
        .padding()
    }
}

#Preview("Relay list — mixed states") {
    NostrRelayListPreview()
}

#Preview("Relay list — empty state") {
    NostrRelayList(
        relays: [],
        relayRoleOptions: []
    )
    .padding()
}
