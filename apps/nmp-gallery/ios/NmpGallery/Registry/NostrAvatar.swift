import SwiftUI

/// Circular avatar for a Nostr profile. Shows the remote picture when
/// available; falls back to a deterministic identicon derived from `pubkey`.
///
/// Replace `AsyncImage` with your own image cache (Kingfisher, Nuke, etc.)
/// if you already have one — the identicon fallback is self-contained.
///
/// Depends on `swiftui/user-avatar` for `ProfileWire`.
public struct NostrAvatar: View {
    public let pubkey: String
    public let pictureUrl: URL?
    public let size: CGFloat

    public init(pubkey: String, pictureUrl: URL? = nil, size: CGFloat = 40) {
        self.pubkey = pubkey
        self.pictureUrl = pictureUrl
        self.size = size
    }

    public init(profile: ProfileWire, size: CGFloat = 40) {
        self.pubkey = profile.pubkey
        self.pictureUrl = profile.avatarURL
        self.size = size
    }

    public var body: some View {
        Group {
            if let url = pictureUrl {
                AsyncImage(url: url) { phase in
                    switch phase {
                    case .success(let image):
                        image.resizable().scaledToFill()
                    default:
                        identicon
                    }
                }
            } else {
                identicon
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .accessibilityHidden(true)
    }

    private var identicon: some View {
        ZStack {
            Circle().fill(NostrIdenticon.color(forPubkey: pubkey))
            Text(NostrIdenticon.initials(forPubkey: pubkey))
                .font(.system(size: size * 0.35, weight: .semibold))
                .foregroundStyle(.white)
        }
    }
}

// MARK: - Identicon
//
// NOTE (gallery local edit): the `NostrIdenticon` enum that originally lived
// here in the upstream `swiftui/user-avatar` registry component has been
// removed. The richer version defined in `ContentTreeWire.swift`
// (`swiftui/content-core`) provides the same `color(forPubkey:)` and
// `initials(forPubkey:)` API plus the geometric `identiconView(...)`
// used by `NostrMentionChip` and `NostrQuoteCard`. Keeping both definitions
// in the same Swift module is a redeclaration error.
//
// The task's literal instruction was to dedup the OTHER direction (strip
// from ContentTreeWire.swift), but that would remove `identiconView(...)`
// — which the other registry components call. The dedup was inverted here
// after verifying the call sites; see the PR body for the rationale.
