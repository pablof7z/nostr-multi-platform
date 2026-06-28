import SwiftUI

/// Note row used inside ThreadScreen. Supports a "focused" state
/// (the event the thread was opened on) which gives it a hairline accent
/// leading edge and slightly more visual weight.
struct ThreadNoteRow: View {
    let card: ChirpEventCard
    let isFocused: Bool
    let eventCards: [String: ChirpEventCard]
    let onAvatarTap: () -> Void
    let onLike: () -> Void
    let onReply: () -> Void
    var onRepost: (() -> Void)? = nil

    @EnvironmentObject private var model: KernelModel
    @State private var likeTapped = false

    private var authorDisplayLabel: String {
        model.profile(forPubkey: card.authorPubkey)?.display
            ?? card.authorDisplayName
            ?? card.authorPubkey.shortHex
    }

    /// Icon-only action glyph sized to match the home-feed action bar (15pt
    /// regular symbol inside a 44×32 hit target).
    private func threadActionLabel(icon: String) -> some View {
        Image(systemName: icon)
            .font(.system(size: 15, weight: .regular))
            .frame(minWidth: 44, minHeight: 32, alignment: .leading)
    }

    var body: some View {
        // The focused (detail) note earns emphasis through a larger avatar and
        // heavier title weight — not a tinted card or a bright accent rail,
        // which read as "selected chrome" and cheapen the screen. This mirrors
        // how native detail views (Mail, X's detail tweet) present the anchor.
        HStack(alignment: .top, spacing: 8) {
            Button(action: onAvatarTap) {
                NostrAvatar(
                    pubkey: card.authorPubkey,
                    url: card.authorPictureUrl,
                    size: isFocused ? 46 : 38
                )
                .equatable()
            }
            .buttonStyle(.plain)

            noteBodyContent
        }
        .padding(.vertical, isFocused ? 12 : 8)
        .padding(.horizontal, 16)
    }

    // ── Body column (header + content + actions) ──────────────────────────
    //
    // Extracted as a non-`@ViewBuilder` `some View` so the kind:6 repost
    // branching can use `let` bindings instead of trying to mix declarations
    // with view builders inside the parent `HStack`.

    private var noteBodyContent: some View {
        let isRepost = card.isRepost
        let context = NoteRenderContext(eventCards: eventCards)
        let displayContent = card.contentPreview.isEmpty ? card.content : card.contentPreview
        return VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 4) {
                Text(authorDisplayLabel)
                    .font(isFocused ? .headline : .callout)
                    .fontWeight(isFocused ? .semibold : .regular)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Spacer()
                Text(card.createdAt.relativeTimeFromUnixSeconds)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            if isRepost {
                HStack(spacing: 3) {
                    Image(systemName: "arrow.2.squarepath")
                        .font(.system(size: 11, weight: .medium))
                    Text("Repost")
                        .font(.caption)
                }
                .foregroundStyle(.secondary)
            }

            if !displayContent.isEmpty {
                NoteContentView(
                    content: displayContent,
                    contentTree: card.contentTree,
                    renderContext: context,
                    font: isFocused ? .body : .callout
                )
                .foregroundStyle(.primary)
                .padding(.bottom, isFocused ? 4 : 0)
            }

            // Action row — same icon set, order, sizing and colors as the
            // home-feed `NoteActionsRow` so a note reads identically wherever
            // it appears: reply, repost, like.
            HStack(spacing: 28) {
                Button(action: onReply) {
                    threadActionLabel(icon: "bubble.left")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Reply")

                if let onRepost {
                    Button {
                        onRepost()
                        UIImpactFeedbackGenerator(style: .light).impactOccurred()
                    } label: {
                        threadActionLabel(icon: "arrow.2.squarepath")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Repost")
                }

                Button {
                    guard !likeTapped else { return }
                    likeTapped = true
                    onLike()
                    UIImpactFeedbackGenerator(style: .soft).impactOccurred()
                } label: {
                    threadActionLabel(icon: likeTapped ? "heart.fill" : "heart")
                        .foregroundStyle(likeTapped ? ChirpColor.accent : .secondary)
                        .scaleEffect(likeTapped ? 1.15 : 1.0)
                        .animation(.spring(response: 0.25, dampingFraction: 0.4), value: likeTapped)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Like")
            }
            .padding(.top, 6)
        }
    }

}
