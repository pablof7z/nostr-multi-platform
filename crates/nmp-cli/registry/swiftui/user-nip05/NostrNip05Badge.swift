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
/// Depends on `swiftui/user-core` for `ProfileWire`.
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

    public var body: some View {
        HStack(spacing: 4) {
            Image(systemName: "checkmark.seal.fill")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(Color.accentColor)
            Text(nip05)
                .font(.callout)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Verified: \(nip05)")
    }
}
