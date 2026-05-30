import SwiftUI

/// NIP-05 verified identity badge — checkmark icon + identifier string.
///
/// Renders nothing when `profile.nip05` is nil or empty.
/// The failable init lets you gate the badge in one expression:
///
/// ```swift
/// if let badge = NostrNip05Badge(profile: profile) { badge }
/// ```
///
/// Depends on `swiftui/user-avatar` for `ProfileWire`.
public struct NostrNip05Badge: View {
    public let nip05: String

    /// Returns `nil` when the profile has no NIP-05 identifier.
    public init?(profile: ProfileWire) {
        guard let v = profile.nip05, !v.isEmpty else { return nil }
        self.nip05 = v
    }

    public init(nip05: String) {
        self.nip05 = nip05
    }

    /// Display form of the NIP-05 identifier. The root identifier `_@domain`
    /// is shown as just `domain` per the NIP-05 convention — never the raw
    /// `_@` (matrix rule). A normal `name@domain` is shown verbatim.
    var displayLabel: String {
        if nip05.hasPrefix("_@") {
            return String(nip05.dropFirst(2))
        }
        return nip05
    }

    public var body: some View {
        HStack(spacing: 4) {
            Image(systemName: "checkmark.seal.fill")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(Color.accentColor)
            Text(displayLabel)
                .font(.callout)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Verified: \(displayLabel)")
    }
}
