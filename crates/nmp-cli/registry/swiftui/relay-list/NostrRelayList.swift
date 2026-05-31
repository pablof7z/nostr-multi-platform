import SwiftUI

// MARK: - Wire types

/// One row of the kernel's `projections.relay_edit_rows` array.
///
/// `roleLabel` and `roleTint` are pre-formatted by the kernel
/// (`crates/nmp-core/src/actor/relay_roles.rs`) — do not reformat them
/// in Swift (aim.md §6.9 / display separation rule).
///
/// `roleTint` is a semantic token (`accent` | `info` | `success` |
/// `neutral`) emitted by `RELAY_ROLE_METADATA`. A 6-char hex string is
/// also accepted to stay forward-compatible if the kernel ever emits
/// custom palette colours; see `NostrRelayList.tintColor(for:)`.
public struct NostrRelayEditRow: Codable, Identifiable, Equatable, RenderIdentifiable {
    public var id: String { url }
    public let url: String
    public let role: String
    public let roleLabel: String
    public let roleTint: String

    public init(url: String, role: String, roleLabel: String, roleTint: String) {
        self.url = url
        self.role = role
        self.roleLabel = roleLabel
        self.roleTint = roleTint
    }

    private enum CodingKeys: String, CodingKey {
        case url
        case role
        case roleLabel = "role_label"
        case roleTint = "role_tint"
    }

    public func rendersIdentically(_ other: Self) -> Bool {
        self.url == other.url
            && self.role == other.role
            && self.roleLabel == other.roleLabel
            && self.roleTint == other.roleTint
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

/// Row model for the relay list ForEach, bundling relay + connection status
/// so that EquatableRow sees the full render state when connection status changes.
private struct RelayListRowModel: RenderIdentifiable {
    let relay: NostrRelayEditRow
    let connection: String?

    func rendersIdentically(_ other: Self) -> Bool {
        relay.rendersIdentically(other.relay) && connection == other.connection
    }
}

/// Relay list component — shows a user's configured relays with
/// connection-status dots and role badges.
///
/// Mirrors NDK's svelte `RelayList`. Data comes straight from the NMP
/// snapshot: rows from `projections.relay_edit_rows`, connection
/// statuses folded from the top-level `relay_statuses` field keyed by
/// relay URL.
public struct NostrRelayList: View {
    public let relays: [NostrRelayEditRow]
    /// Keyed by relay URL — caller merges from `relay_statuses` (typically
    /// `Dictionary(uniqueKeysWithValues: snapshot.relayStatuses.map { ($0.relayUrl, $0.connection) })`).
    public var connectionStatus: [String: String]
    public var onRelayTap: ((NostrRelayEditRow) -> Void)?

    public init(
        relays: [NostrRelayEditRow],
        connectionStatus: [String: String] = [:],
        onRelayTap: ((NostrRelayEditRow) -> Void)? = nil
    ) {
        self.relays = relays
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
                    EquatableRow(model: RelayListRowModel(relay: relay, connection: connectionStatus[relay.url])) { m in
                        RelayRow(
                            relay: m.relay,
                            connection: m.connection,
                            onTap: onRelayTap.map { handler in { handler(m.relay) } }
                        )
                    }
                    .equatable()
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
    let connection: String?
    let onTap: (() -> Void)?

    var body: some View {
        HStack(spacing: 10) {
            ConnectionDot(status: connection)

            Text(displayUrl)
                .font(.body.monospaced())
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: .infinity, alignment: .leading)

            RoleBadge(
                label: relay.roleLabel,
                tint: NostrRelayList.tintColor(for: relay.roleTint)
            )
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 12)
        .contentShape(Rectangle())
        .onTapGesture { onTap?() }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(displayUrl), \(relay.roleLabel), \(accessibilityStatus)")
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
