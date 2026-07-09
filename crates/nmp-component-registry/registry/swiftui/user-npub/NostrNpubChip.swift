import SwiftUI
import UIKit

/// Tappable chip that shows the truncated npub and copies the full
/// bech32 `npub1…` to the clipboard on tap.
///
/// `npub` must come from the kernel projection (the canonical Rust bech32
/// NIP-19 encoder) — never re-derive the encoding in Swift (aim.md §6.9).
/// `npubShort` is a pure string truncation of `npub` — a display decision the
/// host owns (#3098) — see `ProfileWire.npubShort`.
///
/// Depends on `swiftui/user-avatar` for `ProfileWire`.
public struct NostrNpubChip: View {
    public let npub: String
    public let npubShort: String

    @State private var copied = false
    @State private var copiedResetTask: Task<Void, Never>?

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
        .onDisappear {
            copiedResetTask?.cancel()
            copiedResetTask = nil
        }
    }

    private func copyNpub() {
        UIPasteboard.general.string = npub
        copied = true
        copiedResetTask?.cancel()
        copiedResetTask = Task { @MainActor in
            try? await Task.sleep(for: .seconds(2))
            guard !Task.isCancelled else { return }
            copied = false
            copiedResetTask = nil
        }
    }
}
