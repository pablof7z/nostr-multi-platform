import SwiftUI

// MARK: - relay-list

/// Renders the relay list component with current relay component rows covering
/// every role/tint token and every connection-status dot variant.
struct RelayListPage: View {

    @Environment(GalleryModel.self) private var model

    /// Resolve each showcase relay's role into the kernel-emitted
    /// `label`/`tint` tokens from `projections.relay_role_options` (issue
    /// #996). No role→label/tint derivation happens in Swift; the page just
    /// looks the role up in the kernel's options. While the kernel options are
    /// still in flight the
    /// row falls back to the raw role token with an `accent` tint.
    private var relayRows: [NostrRelayEditRow] {
        GALLERY_SHOWCASE.relays.map { relay in
            let option = model.relayRoleOptions.first { $0.value == relay.role }
            return NostrRelayEditRow(
                url: relay.url,
                role: relay.role,
                roleLabel: option?.label ?? relay.role,
                roleTint: option?.tint ?? "accent"
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
