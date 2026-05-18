import SwiftUI

/// Note row used inside ThreadScreen. Supports a "focused" state
/// (the event the thread was opened on) which gives it a hairline accent
/// leading edge and slightly more visual weight.
struct ThreadNoteRow: View {
    let item: TimelineItem
    let isFocused: Bool
    let onAvatarTap: () -> Void
    let onLike: () -> Void
    let onReply: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            // Accent hairline for focused note
            if isFocused {
                Rectangle()
                    .fill(ChirpColor.accent)
                    .frame(width: 2)
                    .cornerRadius(1)
                    .padding(.vertical, ChirpSpace.xs)
            }

            HStack(alignment: .top, spacing: ChirpSpace.m) {
                Button(action: onAvatarTap) {
                    ChirpAvatar(
                        url: item.authorPictureUrl,
                        initials: item.authorAvatarInitials,
                        colorHex: item.authorAvatarColor,
                        size: isFocused ? 46 : 38
                    )
                }
                .buttonStyle(.plain)
                .animation(.smooth(duration: 0.2), value: isFocused)

                VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                    HStack(spacing: ChirpSpace.s) {
                        Text(item.authorDisplay)
                            .font(isFocused ? ChirpFont.headline : ChirpFont.callout)
                            .fontWeight(isFocused ? .semibold : .regular)
                            .foregroundStyle(ChirpColor.textPrimary)
                            .lineLimit(1)
                        Spacer()
                        Text(item.createdAtDisplay)
                            .font(ChirpFont.caption)
                            .foregroundStyle(ChirpColor.textTertiary)
                    }

                    Text(item.content)
                        .font(isFocused ? ChirpFont.body : ChirpFont.callout)
                        .foregroundStyle(ChirpColor.textPrimary)
                        .fixedSize(horizontal: false, vertical: true)
                        .multilineTextAlignment(.leading)
                        .padding(.bottom, isFocused ? ChirpSpace.xs : 0)

                    // Action row
                    HStack(spacing: ChirpSpace.xl) {
                        Button(action: onLike) {
                            Label("Like", systemImage: "heart")
                                .font(ChirpFont.caption)
                                .foregroundStyle(ChirpColor.like.opacity(0.6))
                                .labelStyle(.iconOnly)
                        }
                        .buttonStyle(.plain)

                        Button(action: onReply) {
                            Label("Reply", systemImage: "arrowshape.turn.up.left")
                                .font(ChirpFont.caption)
                                .foregroundStyle(ChirpColor.accent.opacity(0.7))
                                .labelStyle(.iconOnly)
                        }
                        .buttonStyle(.plain)

                        if item.relayCount > 0 {
                            HStack(spacing: ChirpSpace.xs) {
                                Image(systemName: "antenna.radiowaves.left.and.right")
                                    .font(ChirpFont.caption)
                                Text("\(item.relayCount)")
                                    .font(ChirpFont.caption)
                            }
                            .foregroundStyle(ChirpColor.textTertiary)
                        }
                    }
                    .padding(.top, ChirpSpace.xs)
                }
            }
            .padding(.vertical, isFocused ? ChirpSpace.m : ChirpSpace.s)
            .padding(.horizontal, ChirpSpace.l)
        }
        .background(isFocused ? ChirpColor.accentSoft : Color.clear)
        .animation(.smooth(duration: 0.25), value: isFocused)
    }
}
