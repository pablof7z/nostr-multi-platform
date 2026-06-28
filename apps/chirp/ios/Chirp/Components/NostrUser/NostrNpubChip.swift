import SwiftUI
import UIKit

/// Tappable chip that shows the Rust-truncated npub and copies the full
/// bech32 `npub1…` to the clipboard on tap.
///
/// Registry component `swiftui/user-npub`, installed into Chirp.
///
/// `npub` and `npubShort` must come from the kernel projection — never
/// reformat them in Swift (aim.md §6.9). Chirp's `ProfileWire.npub` is
/// optional (ADR-0032/V-115: bech32 is no longer carried by every projection),
/// so `init?(profile:)` is failable: it returns `nil` when the full bech32
/// `npub` is absent and the caller has nothing copyable to offer.
struct NostrNpubChip: View {
    let npub: String
    let npubShort: String

    @State private var copied = false

    /// Returns `nil` when the profile carries no full bech32 `npub`.
    init?(profile: ProfileWire) {
        guard let full = profile.npub, !full.isEmpty else { return nil }
        self.npub = full
        self.npubShort = profile.npubShort
    }

    init(npub: String, npubShort: String) {
        self.npub = npub
        self.npubShort = npubShort
    }

    var body: some View {
        Button(action: copyNpub) {
            HStack(spacing: 4) {
                Text(npubShort)
                    .font(.body.monospaced())
                    .foregroundStyle(.secondary)
                Image(systemName: copied ? "checkmark" : "doc.on.doc")
                    .font(.system(size: 13))
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
