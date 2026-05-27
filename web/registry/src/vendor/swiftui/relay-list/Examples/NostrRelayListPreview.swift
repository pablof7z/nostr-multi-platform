import SwiftUI

struct NostrRelayListPreview: View {
    var body: some View {
        NostrRelayList(
            relays: [
                NostrRelayEditRow(
                    url: "wss://relay.damus.io",
                    role: "both",
                    roleLabel: "Both",
                    roleTint: "accent"
                ),
                NostrRelayEditRow(
                    url: "wss://nos.lol",
                    role: "read",
                    roleLabel: "Read",
                    roleTint: "info"
                ),
                NostrRelayEditRow(
                    url: "wss://relay.snort.social",
                    role: "write",
                    roleLabel: "Write",
                    roleTint: "success"
                ),
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
    NostrRelayList(relays: [])
        .padding()
}
