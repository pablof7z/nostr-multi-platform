import SwiftUI

// MARK: - Wire types

/// One row of the kernel's `projections.relay_edit_rows` array.
///
/// The kernel emits only `url` and `role` (canonical string). Role label
/// and tint are looked up from the `relay_role_options` projection by the
/// component that displays this row.
public struct NostrRelayEditRow: Codable, Identifiable, Equatable {
    public var id: String { url }
    public let url: String
    public let role: String

    public init(url: String, role: String) {
        self.url = url
        self.role = role
    }

    private enum CodingKeys: String, CodingKey {
        case url
        case role
    }
}

/// One entry of the kernel's top-level `relay_statuses` snapshot field
/// (i.e. `snapshot.relay_statuses[]`, not nested inside `projections`).
///
/// `connection` is one of `connected | connecting | disconnected |
/// error` (closed token set). Callers typically fold the array into a
/// `[relay_url: connection]` dictionary before handing it to
/// `NostrRelayList`.
public struct NostrRelayConnectionStatus: Codable, Equatable {
    public let relayUrl: String
    public let connection: String
    public let reconnectCount: UInt32

    public init(relayUrl: String, connection: String, reconnectCount: UInt32) {
        self.relayUrl = relayUrl
        self.connection = connection
        self.reconnectCount = reconnectCount
    }

    private enum CodingKeys: String, CodingKey {
        case relayUrl = "relay_url"
        case connection
        case reconnectCount = "reconnect_count"
    }
}

// MARK: - Component

/// Role option metadata — one entry from `projections.relay_role_options`.
/// Used internally by `NostrRelayList` to look up role label and tint.
public struct RelayRoleOption: Codable, Identifiable, Equatable {
    public let value: String
    public let label: String
    public let tint: String
    public let isDefault: Bool

    public var id: String { value }

    public init(value: String, label: String, tint: String, isDefault: Bool) {
        self.value = value
        self.label = label
        self.tint = tint
        self.isDefault = isDefault
    }

    private enum CodingKeys: String, CodingKey {
        case value
        case label
        case tint
        case isDefault = "is_default"
    }
}

/// Relay list component — shows a user's configured relays with
/// connection-status dots and role badges.
///
/// Mirrors NDK's svelte `RelayList`. Data comes straight from the NMP
/// snapshot: rows from `projections.relay_edit_rows`, role options from
/// `projections.relay_role_options`, and connection statuses folded from
/// the top-level `relay_statuses` field keyed by relay URL.
public struct NostrRelayList: View {
    public let relays: [NostrRelayEditRow]
    public let relayRoleOptions: [RelayRoleOption]
    /// Keyed by relay URL — caller merges from `relay_statuses` (typically
    /// `Dictionary(uniqueKeysWithValues: snapshot.relayStatuses.map { ($0.relayUrl, $0.connection) })`).
    public var connectionStatus: [String: String]
    public var onRelayTap: ((NostrRelayEditRow) -> Void)?

    public init(
        relays: [NostrRelayEditRow],
        relayRoleOptions: [RelayRoleOption],
        connectionStatus: [String: String] = [:],
        onRelayTap: ((NostrRelayEditRow) -> Void)? = nil
    ) {
        self.relays = relays
        self.relayRoleOptions = relayRoleOptions
        self.connectionStatus = connectionStatus
        self.onRelayTap = onRelayTap
    }

    public var body: some View {
        if relays.isEmpty {
            VStack {
                Text("No relays configured")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 24)
        } else {
            VStack(spacing: 0) {
                ForEach(relays) { relay in
                    RelayRow(
                        relay: relay,
                        relayRoleOptions: relayRoleOptions,
                        connection: connectionStatus[relay.url],
                        onTap: onRelayTap.map { handler in { handler(relay) } }
                    )
                }
            }
        }
    }

    // MARK: Internals

    /// Resolve a relay-role tint token (or fallback hex) into a `Color`.
    ///
    /// The kernel currently emits semantic tokens (`accent`, `info`,
    /// `success`, `neutral`) — those are checked first. A 6-char hex
    /// string is also accepted via `Color(hex:)` to stay
    /// forward-compatible. Anything unrecognised falls back to
    /// `.secondary`.
    static func tintColor(for token: String) -> Color {
        switch token.lowercased() {
        case "accent": return .accentColor
        case "info": return .blue
        case "success": return .green
        case "warning": return .orange
        case "danger", "error": return .red
        case "neutral": return .secondary
        default:
            return Color(hex: token) ?? .secondary
        }
    }
}

// MARK: - Row

private struct RelayRow: View {
    let relay: NostrRelayEditRow
    let relayRoleOptions: [RelayRoleOption]
    let connection: String?
    let onTap: (() -> Void)?

    private var roleOption: RelayRoleOption? {
        relayRoleOptions.first { $0.value == relay.role }
    }

    private var roleLabel: String {
        roleOption?.label ?? relay.role
    }

    private var roleTint: String {
        roleOption?.tint ?? "accent"
    }

    var body: some View {
        HStack(spacing: 10) {
            ConnectionDot(status: connection)

            Text(displayUrl)
                .font(.body.monospaced())
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: .infinity, alignment: .leading)

            RoleBadge(
                label: roleLabel,
                tint: NostrRelayList.tintColor(for: roleTint)
            )
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 12)
        .contentShape(Rectangle())
        .onTapGesture { onTap?() }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(displayUrl), \(roleLabel), \(accessibilityStatus)")
        .accessibilityAddTraits(onTap != nil ? .isButton : [])
    }

    private var displayUrl: String {
        if relay.url.hasPrefix("wss://") {
            return String(relay.url.dropFirst("wss://".count))
        }
        if relay.url.hasPrefix("ws://") {
            return String(relay.url.dropFirst("ws://".count))
        }
        return relay.url
    }

    private var accessibilityStatus: String {
        switch connection {
        case "connected": return "connected"
        case "connecting": return "connecting"
        case "error": return "error"
        case "disconnected": return "disconnected"
        default: return "status unknown"
        }
    }
}

// MARK: - Connection dot

private struct ConnectionDot: View {
    let status: String?

    @State private var pulse: Bool = false

    var body: some View {
        Circle()
            .fill(color)
            .frame(width: 8, height: 8)
            .opacity(isConnecting ? (pulse ? 0.4 : 1.0) : 1.0)
            .onAppear {
                guard isConnecting else { return }
                withAnimation(.easeInOut(duration: 0.8).repeatForever(autoreverses: true)) {
                    pulse = true
                }
            }
            .accessibilityHidden(true)
    }

    private var isConnecting: Bool { status == "connecting" }

    private var color: Color {
        switch status {
        case "connected": return .green
        case "connecting": return .orange
        case "error": return .red
        default: return .secondary
        }
    }
}

// MARK: - Role badge

private struct RoleBadge: View {
    let label: String
    let tint: Color

    var body: some View {
        Text(label)
            .font(.caption.weight(.medium))
            .foregroundStyle(.white)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(tint, in: RoundedRectangle(cornerRadius: 4, style: .continuous))
    }
}

// MARK: - Color(hex:)

private extension Color {
    /// Parse a 6-character RGB hex string (optionally prefixed with `#`).
    /// Returns `nil` if the input is not a valid 6-char hex.
    init?(hex: String) {
        let cleaned = hex.hasPrefix("#") ? String(hex.dropFirst()) : hex
        guard cleaned.count == 6,
              let rgb = UInt64(cleaned, radix: 16) else { return nil }
        self.init(
            red:   Double((rgb >> 16) & 0xFF) / 255,
            green: Double((rgb >>  8) & 0xFF) / 255,
            blue:  Double( rgb        & 0xFF) / 255
        )
    }
}
