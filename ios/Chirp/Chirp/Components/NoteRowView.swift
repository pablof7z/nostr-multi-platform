import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// NoteRowView — polished timeline cell for the Home feed.
//
// Tap targets:
//   • avatar  → router.push(.profile)
//   • whole row → router.push(.thread)
//   • action buttons (reply, repost, like, zap) → kernel commands / sheets
//
// Button nesting strategy: every inner interactive element uses
// .buttonStyle(.borderless) so its tap doesn't propagate to the row-level
// Button wrapper. The row itself is a plain Button with .contentShape so the
// entire non-button area navigates to the thread.
// ─────────────────────────────────────────────────────────────────────────

struct NoteRowView: View {
    let item: TimelineItem

    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    /// Controls the inline reply sheet for this row.
    @State private var showReply = false
    /// Transient like-animation state.
    @State private var likeTapped = false

    var body: some View {
        Button {
            router.push(.thread(eventID: item.id))
        } label: {
            VStack(alignment: .leading, spacing: 0) {
                rowContent
                NoteActionsRow(
                    item: item,
                    likeTapped: $likeTapped,
                    showReply: $showReply
                )
                .padding(.top, ChirpSpace.m)
                // hairline divider lives at the bottom via listRowSeparator(hidden)
                // + an explicit Divider here for full-bleed styling
                Divider()
                    .background(ChirpColor.hairline)
                    .padding(.top, ChirpSpace.m)
            }
            .padding(.vertical, ChirpSpace.m)
            .padding(.horizontal, ChirpSpace.l)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .listRowInsets(EdgeInsets())
        .listRowSeparator(.hidden)
        .listRowBackground(Color.clear)
        .sheet(isPresented: $showReply) {
            ComposeView(replyToID: item.id)
        }
    }

    private var rowContent: some View {
        HStack(alignment: .top, spacing: ChirpSpace.m) {
            avatarButton

            VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                authorHeader
                noteContent
                relayChip
            }
        }
    }

    // ── Avatar (taps to profile) ──────────────────────────────────────────

    private var avatarButton: some View {
        Button {
            router.push(.profile(pubkey: item.authorPubkey))
        } label: {
            ChirpAvatar(
                url: item.authorPictureUrl,
                initials: item.authorAvatarInitials,
                colorHex: item.authorAvatarColor,
                size: 44
            )
        }
        .buttonStyle(.borderless)
    }

    // ── Author name + truncated pubkey + timestamp ────────────────────────

    private var authorHeader: some View {
        HStack(alignment: .firstTextBaseline, spacing: ChirpSpace.xs) {
            Text(item.authorDisplay)
                .font(ChirpFont.headline)
                .foregroundStyle(ChirpColor.textPrimary)
                .lineLimit(1)

            Text(shortPubkey(item.authorPubkey))
                .font(ChirpFont.mono)
                .foregroundStyle(ChirpColor.textTertiary)
                .lineLimit(1)

            Spacer(minLength: 0)

            Text(item.createdAtDisplay)
                .font(ChirpFont.caption)
                .foregroundStyle(ChirpColor.textSecondary)
        }
    }

    // ── Note content ──────────────────────────────────────────────────────

    private var noteContent: some View {
        let (text, isRepost) = effectiveContent(item.content)
        return VStack(alignment: .leading, spacing: ChirpSpace.xs) {
            if isRepost {
                HStack(spacing: 3) {
                    Image(systemName: "arrow.2.squarepath")
                        .font(.system(size: 11, weight: .medium))
                    Text("Repost")
                        .font(ChirpFont.caption)
                }
                .foregroundStyle(ChirpColor.textTertiary)
            }
            if !text.isEmpty {
                NoteContentView(content: text, font: ChirpFont.body)
                    .foregroundStyle(ChirpColor.textPrimary)
            }
        }
        .padding(.top, ChirpSpace.xs)
    }

    // Kind:6 reposts carry the full reposted-event JSON as their content field.
    // Extract the inner text; treat anything that doesn't parse as plain content.
    private func effectiveContent(_ raw: String) -> (String, Bool) {
        guard raw.hasPrefix("{"),
              let data = raw.data(using: .utf8),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return (raw, false)
        }
        return ((json["content"] as? String) ?? "", true)
    }

    // ── Relay-count chip ──────────────────────────────────────────────────

    @ViewBuilder
    private var relayChip: some View {
        if item.relayCount > 0 {
            HStack(spacing: 3) {
                Image(systemName: "antenna.radiowaves.left.and.right")
                    .font(.system(size: 10, weight: .medium))
                Text("\(item.relayCount)")
                    .font(ChirpFont.caption)
            }
            .foregroundStyle(ChirpColor.textTertiary)
            .padding(.horizontal, ChirpSpace.s)
            .padding(.vertical, 3)
            .background(ChirpColor.surface.opacity(0.6),
                        in: Capsule())
            .padding(.top, ChirpSpace.xs)
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    /// "npub1abc…ef12" style truncation from hex pubkey.
    private func shortPubkey(_ hex: String) -> String {
        guard hex.count >= 12 else { return hex }
        return "\(hex.prefix(6))…\(hex.suffix(4))"
    }
}

// ─────────────────────────────────────────────────────────────────────────
// NoteActionsRow — reply / repost / like / zap action buttons.
// Kept in the same file for cohesion; small enough not to warrant a split.
// ─────────────────────────────────────────────────────────────────────────

struct NoteActionsRow: View {
    let item: TimelineItem
    @Binding var likeTapped: Bool
    @Binding var showReply: Bool

    @EnvironmentObject private var model: KernelModel

    var body: some View {
        HStack(spacing: 0) {
            actionButton(
                icon: "bubble.left",
                label: "Reply",
                color: ChirpColor.textSecondary
            ) {
                showReply = true
            }

            Spacer()

            actionButton(
                icon: "arrow.2.squarepath",
                label: "Repost",
                color: ChirpColor.textSecondary
            ) {
                // Repost command not yet on kernel surface — no-op.
            }

            Spacer()

            likeButton

            Spacer()

            actionButton(
                icon: "bolt",
                label: "Zap",
                color: ChirpColor.zap
            ) {
                // Zap command not yet on kernel surface — no-op.
            }
        }
        .padding(.horizontal, ChirpSpace.xs)
    }

    // ── Like with spring animation + haptic ──────────────────────────────

    private var likeButton: some View {
        Button {
            guard !likeTapped else { return }
            likeTapped = true
            model.react(targetEventID: item.id, reaction: "❤")
            UIImpactFeedbackGenerator(style: .soft).impactOccurred()
        } label: {
            HStack(spacing: 5) {
                Image(systemName: likeTapped ? "heart.fill" : "heart")
                    .font(.system(size: 15, weight: .regular))
                    .foregroundStyle(likeTapped ? ChirpColor.like : ChirpColor.textSecondary)
                    .scaleEffect(likeTapped ? 1.25 : 1.0)
                    .animation(.spring(response: 0.3, dampingFraction: 0.5), value: likeTapped)
            }
            .frame(minWidth: 44, minHeight: 32, alignment: .center)
        }
        .buttonStyle(.borderless)
    }

    // ── Generic action button factory ────────────────────────────────────

    @ViewBuilder
    private func actionButton(
        icon: String,
        label: String,
        color: Color,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: 15, weight: .regular))
                .foregroundStyle(color)
                .frame(minWidth: 44, minHeight: 32, alignment: .center)
        }
        .buttonStyle(.borderless)
        .accessibilityLabel(label)
    }
}

// Previews omitted — KernelModel init requires the nmp_core FFI static lib
// which is not linked in the Xcode Preview host; previewing would crash.
// Test visually by running on simulator/device.
