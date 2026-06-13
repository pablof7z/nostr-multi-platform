import SwiftUI

/// Note row used inside ThreadScreen. Supports a "focused" state
/// (the event the thread was opened on) which gives it a hairline accent
/// leading edge and slightly more visual weight.
struct ThreadNoteRow: View {
    let card: ChirpEventCard
    let isFocused: Bool
    let mentionProfiles: [String: MentionProfile]
    let eventCards: [String: ChirpEventCard]
    let timelineItems: [String: TimelineItem]
    let onAvatarTap: () -> Void
    let onLike: () -> Void
    let onReply: () -> Void
    var onRepost: (() -> Void)? = nil

    @EnvironmentObject private var model: KernelModel
    @State private var likeTapped = false

    private var authorDisplayLabel: String {
        model.profile(forPubkey: card.authorPubkey)?.display
            ?? card.authorDisplayName
            ?? mentionProfiles[card.authorPubkey]?.display
            ?? card.authorPubkey.shortHex
    }

    private var authorAvatarInitials: String {
        let name = model.profile(forPubkey: card.authorPubkey)?.display
            ?? card.authorDisplayName
        return (name ?? card.authorPubkey).displayInitials
    }

    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            // Accent hairline for focused note
            if isFocused {
                Rectangle()
                    .fill(ChirpColor.accent)
                    .frame(width: 2)
                    .cornerRadius(1)
                    .padding(.vertical, 4)
            }

            HStack(alignment: .top, spacing: 8) {
                Button(action: onAvatarTap) {
                    NostrAvatar(
                        pubkey: card.authorPubkey,
                        url: card.authorPictureUrl,
                        initials: authorAvatarInitials,
                        colorHex: card.authorPubkey.pubkeyColorHex,
                        size: isFocused ? 46 : 38
                    )
                }
                .buttonStyle(.plain)

                noteBodyContent
            }
            .padding(.vertical, isFocused ? 12 : 8)
            .padding(.horizontal, 16)
        }
        .background(isFocused ? ChirpColor.focusedBackground : ChirpColor.transparent)
    }

    // ── Body column (header + content + actions) ──────────────────────────
    //
    // Extracted as a non-`@ViewBuilder` `some View` so the kind:6 repost
    // branching can use `let` bindings instead of trying to mix declarations
    // with view builders inside the parent `HStack`.

    private var noteBodyContent: some View {
        let isRepost = card.isRepost
        let context = NoteRenderContext(
            mentionProfiles: mentionProfiles,
            eventCards: eventCards,
            timelineItems: timelineItems
        )
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

            // Action row
            HStack(spacing: 24) {
                Button {
                    guard !likeTapped else { return }
                    likeTapped = true
                    onLike()
                    UIImpactFeedbackGenerator(style: .soft).impactOccurred()
                } label: {
                    Image(systemName: likeTapped ? "heart.fill" : "heart")
                        .font(.caption)
                        .foregroundStyle(likeTapped ? ChirpColor.like : ChirpColor.textSecondary)
                        .scaleEffect(likeTapped ? 1.35 : 1.0)
                        .animation(.spring(response: 0.25, dampingFraction: 0.4), value: likeTapped)
                }
                .buttonStyle(.plain)

                Button(action: onReply) {
                    Label("Reply", systemImage: "arrowshape.turn.up.left")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .labelStyle(.iconOnly)
                }
                .buttonStyle(.plain)

                if let onRepost {
                    Button {
                        onRepost()
                        UIImpactFeedbackGenerator(style: .light).impactOccurred()
                    } label: {
                        Label("Repost", systemImage: "arrow.2.squarepath")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .labelStyle(.iconOnly)
                    }
                    .buttonStyle(.plain)
                }

            }
            .padding(.top, 4)
        }
    }

}
