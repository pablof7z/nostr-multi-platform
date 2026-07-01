import SwiftUI

// MARK: - Wire types

/// One row of the kernel's `projections.configured_relays` array.
///
/// The kernel emits the canonical role token (`both`, `read`, `write`,
/// `indexer`, `both,indexer`, …). The Swift shell maps that role to label and
/// color locally.
public struct NostrRelayEditRow: Codable, Identifiable, Equatable, Sendable, RenderIdentifiable {
    public var id: String { url }
    public let url: String
    public let role: String

    public init(url: String, role: String) {
        self.url = url
        self.role = role
    }

    public func rendersIdentically(_ other: Self) -> Bool {
        self.url == other.url
            && self.role == other.role
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
private struct RelayListRowModel: RenderIdentifiable, Sendable {
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
/// snapshot: rows from `projections.configured_relays`, connection statuses
/// folded from the top-level `relay_statuses` field keyed by relay URL, with
/// role presentation derived locally.
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
                        NostrRelayRow(
                            url: m.relay.url,
                            role: m.relay.role,
                            connection: m.connection,
                            onTap: onRelayTap.map { handler in { handler(m.relay) } }
                        )
                    }
                    .equatable()
                }
            }
        }
    }
}

// MARK: - Row primitive

/// The base relay-row primitive: a connection-status dot, a monospaced relay
/// URL, and a role badge.
///
/// Takes the kernel-emitted role token and maps it to SwiftUI presentation
/// locally.
public struct NostrRelayRow: View {
    public let url: String
    public let role: String
    public let connection: String?
    public let onTap: (() -> Void)?

    public init(
        url: String,
        role: String,
        connection: String? = nil,
        onTap: (() -> Void)? = nil
    ) {
        self.url = url
        self.role = role
        self.connection = connection
        self.onTap = onTap
    }

    public var body: some View {
        HStack(spacing: 10) {
            ConnectionDot(status: connection)

            Text(displayUrl)
                .font(.body.monospaced())
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: .infinity, alignment: .leading)

            RoleBadge(
                label: roleLabel,
                tint: roleTint
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
        if url.hasPrefix("wss://") {
            return String(url.dropFirst("wss://".count))
        }
        if url.hasPrefix("ws://") {
            return String(url.dropFirst("ws://".count))
        }
        return url
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

    private var roleLabel: String {
        if role.contains("both") && role.contains("indexer") { return "Both + Indexer" }
        if role.contains("indexer") { return "Indexer" }
        if role.contains("both") { return "Both" }
        return role.prefix(1).uppercased() + String(role.dropFirst())
    }

    private var roleTint: Color {
        if role.contains("both") { return .green }
        if role.contains("indexer") { return .blue }
        return .accentColor
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
