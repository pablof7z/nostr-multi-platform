import SwiftUI

/// Inline display-name text for a Nostr profile.
///
/// Registry component `swiftui/user-name`, installed into Chirp.
///
/// Two construction modes, both rendering-only (no name resolution lives in
/// this leaf):
///
///   * `init(profile:)` — renders `profile.display` (`displayName` when set,
///     else the Rust-truncated `npubShort`). The simple single-projection
///     case used by profile headers.
///   * `init(displayName:)` — renders a label the *caller* already resolved.
///     Chirp's timeline rows resolve the author label across several kernel
///     projections (claimed / resolved / timeline-baked / event-card — the
///     PR #823 flicker fix in `NoteRowView.resolveAuthorLabel`). That
///     multi-source resolution is a data concern that belongs in the app, not
///     in a rendering leaf, so the resolved string is passed straight in.
///
/// This `displayName:` initializer is an upstream-friendly extension to the
/// registry component: any consumer that resolves a label out-of-band can now
/// render it through the shared component instead of re-implementing the text
/// styling, without coupling the component to any one app's resolution chain.
struct NostrProfileName: View {
    private let label: String
    var font: Font
    var color: Color

    init(
        profile: ProfileWire,
        font: Font = .headline,
        color: Color = .primary
    ) {
        self.label = profile.display
        self.font = font
        self.color = color
    }

    /// Render a pre-resolved display label (see type doc).
    init(
        displayName: String,
        font: Font = .headline,
        color: Color = .primary
    ) {
        self.label = displayName
        self.font = font
        self.color = color
    }

    var body: some View {
        Text(label)
            .font(font)
            .foregroundStyle(color)
            .lineLimit(1)
            .accessibilityLabel("Display name: \(label)")
    }
}
