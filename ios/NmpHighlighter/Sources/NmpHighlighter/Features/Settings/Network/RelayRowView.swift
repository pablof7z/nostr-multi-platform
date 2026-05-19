import Kingfisher
import SwiftUI

/// Single row inside `NetworkSettingsView`. Leads with the relay's NIP-11
/// icon (or a monogram fallback), displays its declared name above the URL,
/// and shows live state + role chips. Chips here are display-only; the
/// detail view makes them tappable.
struct RelayRowView: View {
    let config: RelayConfig
    let diagnostic: RelayDiagnostic?
    let nip11: Nip11Document?

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            RelayAvatar(url: config.url, nip11: nip11, size: 36)
            VStack(alignment: .leading, spacing: 6) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    stateDot
                    Text(primaryLabel)
                        .font(.subheadline.weight(.semibold))
                        .lineLimit(1)
                        .truncationMode(.tail)
                    Spacer()
                    if let rtt = diagnostic?.rttMs {
                        Text("\(rtt) ms")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                }
                Text(displayURL(config.url))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                HStack(spacing: 6) {
                    roleChip("Read", isOn: config.read)
                    roleChip("Write", isOn: config.write)
                    roleChip("Rooms", isOn: config.rooms)
                    roleChip("Indexer", isOn: config.indexer)
                }
            }
        }
        .padding(.vertical, 4)
    }

    // MARK: - Pieces

    /// Prefer the NIP-11 name; fall back to the URL host. Typed as a single
    /// computed property so the row lays out identically whether or not
    /// the probe has resolved yet.
    private var primaryLabel: String {
        if let name = nip11?.name?.trimmingCharacters(in: .whitespaces), !name.isEmpty {
            return name
        }
        return displayURL(config.url)
    }

    @ViewBuilder
    private var stateDot: some View {
        let color: Color = {
            switch diagnostic?.state {
            case .connected: return .green
            case .connecting: return .yellow
            case .disconnected, .terminated, .banned: return .red
            case .none: return .gray
            }
        }()
        Circle()
            .fill(color)
            .frame(width: 8, height: 8)
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

    private func displayURL(_ raw: String) -> String {
        if raw.hasPrefix("wss://") { return String(raw.dropFirst(6)) }
        return raw
    }
}

/// Leading-edge avatar for a relay row. Loads `nip11.icon` via Kingfisher
/// (disk-cached like every other image in the app) with a monogram
/// fallback rendered on the relay's host as a deterministic hue. The
/// fallback also shows while the NIP-11 probe is in flight, so rows look
/// right from the first frame.
struct RelayAvatar: View {
    let url: String
    let nip11: Nip11Document?
    var size: CGFloat = 36

    var body: some View {
        Group {
            if let iconURL = nip11?.icon.flatMap({ URL(string: $0) }) {
                KFImage(iconURL)
                    .resizable()
                    .placeholder { monogram }
                    .fade(duration: 0.2)
                    .cancelOnDisappear(true)
                    .scaledToFill()
            } else {
                monogram
            }
        }
        .frame(width: size, height: size)
        .clipShape(RoundedRectangle(cornerRadius: size / 4, style: .continuous))
    }

    private var monogram: some View {
        ZStack {
            RoundedRectangle(cornerRadius: size / 4, style: .continuous)
                .fill(hueFromHost())
            Text(initial)
                .font(.system(size: size * 0.45, weight: .semibold, design: .rounded))
                .foregroundStyle(.white)
        }
    }

    /// First letter of the relay host (e.g. `r` for `relay.damus.io`).
    private var initial: String {
        let host = url
            .replacingOccurrences(of: "wss://", with: "")
            .replacingOccurrences(of: "ws://", with: "")
        return host.first.map { String($0).uppercased() } ?? "?"
    }

    /// Stable, pleasant fill color derived from the URL's characters.
    /// Avoids a palette lookup or per-host storage — same URL always lands
    /// on the same hue.
    private func hueFromHost() -> Color {
        let seed: Double = url.unicodeScalars.reduce(0) { $0 + Double($1.value) }
        let hue = (seed.truncatingRemainder(dividingBy: 360)) / 360
        return Color(hue: hue, saturation: 0.55, brightness: 0.65)
    }
}
