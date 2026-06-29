import SwiftUI

// MARK: - relay-list

/// Renders the relay list component with current relay component rows covering
/// every relay role and every connection-status dot variant.
struct RelayListPage: View {

    @Environment(GalleryModel.self) private var model

    /// Keep showcase rows aligned with the kernel's canonical role options.
    /// Labels and colors are derived by the Swift relay-list component.
    private var relayRows: [NostrRelayEditRow] {
        GALLERY_SHOWCASE.relays.map { relay in
            let option = model.relayRoleOptions.first { $0.value == relay.role }
            return NostrRelayEditRow(
                url: relay.url,
                role: option?.value ?? relay.role
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

}
