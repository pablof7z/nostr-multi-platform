import SwiftUI

/// Tappable chip that shows the Rust-truncated npub and copies the full
/// bech32 `npub1…` to the clipboard on tap.
///
/// `npub` and `npubShort` must come from the kernel projection —
/// never reformat them in Swift (aim.md §6.9).
///
/// Depends on `swiftui/user-core` for `ProfileWire`.
public struct NostrNpubChip: View {
    public let npub: String
    public let npubShort: String

    @State private var copied = false

    public init(profile: ProfileWire) {
        self.npub = profile.npub
        self.npubShort = profile.npubShort
    }

    public init(npub: String, npubShort: String) {
        self.npub = npub
        self.npubShort = npubShort
    }

    public var body: some View {
        Button(action: copyNpub) {
            HStack(spacing: 4) {
                Text(npubShort)
                    .font(.body.monospaced())
                    .foregroundStyle(.secondary)
                Image(systemName: copied ? "checkmark" : "doc.on.doc")
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(copied ? "Copied" : "Copy npub")
        .accessibilityHint("Double tap to copy full npub to clipboard")
    }

    private func copyNpub() {
        UIPasteboard.general.string = npub
        copied = true
        Task {
            try? await Task.sleep(for: .seconds(2))
            copied = false
        }
    }
}
