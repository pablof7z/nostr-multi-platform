import SwiftUI

/// Row inside `RelaysSettingsView`. Shows a state dot, the relay host, role
/// chips, and a low-priority last-error trailer. Chips are display-only here;
/// the detail view makes them tappable.
struct RelayRowView: View {
    let relay: RelayEditRow
    let status: RelayKernelStatus?

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                stateDot
                Text(displayHost)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
                    .truncationMode(.tail)
                Spacer()
            }
            HStack(spacing: 6) {
                roleChip("Read", isOn: relay.isRead)
                roleChip("Write", isOn: relay.isWrite)
            }
            if let err = status?.lastError, !err.isEmpty {
                Text(err)
                    .font(.caption2)
                    .foregroundStyle(.red)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, 4)
    }

    private var stateDot: some View {
        Circle()
            .fill(dotColor)
            .frame(width: 8, height: 8)
    }

    private var dotColor: Color {
        switch status?.connection.lowercased() {
        case "connected": return .green
        case "connecting", "reconnecting": return .yellow
        case "disconnected", "terminated", "banned", "failed": return .red
        default: return .gray
        }
    }

    private var displayHost: String {
        let raw = relay.url
        if raw.hasPrefix("wss://") { return String(raw.dropFirst(6)) }
        if raw.hasPrefix("ws://")  { return String(raw.dropFirst(5)) }
        return raw
    }

    private func roleChip(_ label: String, isOn: Bool) -> some View {
        Text(label)
            .font(.caption2.weight(.semibold))
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(
                Capsule()
                    .fill(isOn ? Color.accentColor.opacity(0.18) : Color.secondary.opacity(0.12))
            )
            .foregroundStyle(isOn ? Color.accentColor : .secondary)
    }
}
