import SwiftUI

/// Inline display-name text for a Nostr profile.
///
/// Shows `displayName` when set; falls back to `npubShort`
/// (always Rust-formatted — aim.md §6.9).
///
/// Depends on `swiftui/user-core` for `ProfileWire`.
public struct NostrProfileName: View {
    public let profile: ProfileWire
    public var font: Font
    public var color: Color

    public init(
        profile: ProfileWire,
        font: Font = .headline,
        color: Color = .primary
    ) {
        self.profile = profile
        self.font = font
        self.color = color
    }

    public var body: some View {
        Text(profile.display)
            .font(font)
            .foregroundStyle(color)
            .lineLimit(1)
            .accessibilityLabel("Display name: \(profile.display)")
    }
}
