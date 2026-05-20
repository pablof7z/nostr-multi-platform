import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// NoteRowView — polished timeline cell for the Home feed.
//
// Tap targets:
//   • avatar  → router.push(.profile)
//   • whole row → router.push(.thread)
//   • action buttons (reply, like) → kernel commands / sheets
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
                .padding(.top, 8)
            }
            .padding(.vertical, 12)
            .padding(.horizontal, 16)
            .background(ChirpColor.surface)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .sheet(isPresented: $showReply) {
            ComposeView(replyToID: item.id)
        }
    }

    private var rowContent: some View {
        HStack(alignment: .top, spacing: 10) {
            avatarButton

            VStack(alignment: .leading, spacing: 5) {
                authorHeader
                noteContent
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
        .accessibilityIdentifier("timeline-author-link")
    }

    // ── Author name + truncated pubkey + timestamp ────────────────────────

    private var authorHeader: some View {
        VStack(alignment: .leading, spacing: 1) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(item.authorDisplay)
                    .font(.headline.weight(.semibold))
                    .foregroundStyle(.primary)
                    .lineLimit(1)

                Spacer(minLength: 8)

                Text(item.createdAtDisplay)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Text("@\(shortPubkey(item.authorPubkey))")
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
    }

    // ── Note content ──────────────────────────────────────────────────────

    private var noteContent: some View {
        let (text, isRepost) = effectiveContent(item.content)
        return VStack(alignment: .leading, spacing: 4) {
            if isRepost {
                HStack(spacing: 3) {
                    Image(systemName: "arrow.2.squarepath")
                        .font(.system(size: 11, weight: .medium))
                    Text("Repost")
                        .font(.caption)
                }
                .foregroundStyle(.secondary)
            }
            if !text.isEmpty {
                NoteContentView(content: text, font: .body)
                    .foregroundStyle(.primary)
            }
        }
        .padding(.top, 4)
    }

    // Kind:6 reposts carry the full reposted-event JSON as their content field.
    // Extract the inner text; treat anything that doesn't parse as plain content.
    private func effectiveContent(_ raw: String) -> (String, Bool) {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("{"),
              let data = trimmed.data(using: .utf8),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              json["id"] is String,
              json["pubkey"] is String,
              json["kind"] != nil,
              json["sig"] is String,
              let content = json["content"] as? String else {
            return (raw, false)
        }
        return (content, true)
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
        HStack(spacing: 8) {
            actionButton(
                icon: "bubble.left",
                label: "Reply"
            ) {
                showReply = true
            }

            likeButton

            Spacer()
        }
        .padding(.leading, 54)
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
                    .foregroundStyle(likeTapped ? ChirpColor.like : .secondary)
                Text("Like")
                    .font(.caption)
                    .foregroundStyle(likeTapped ? ChirpColor.like : .secondary)
            }
            .frame(minHeight: 32, alignment: .center)
            .padding(.horizontal, 8)
        }
        .buttonStyle(.borderless)
        .accessibilityLabel(likeTapped ? "Liked" : "Like")
    }

    // ── Generic action button factory ────────────────────────────────────

    @ViewBuilder
    private func actionButton(
        icon: String,
        label: String,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            HStack(spacing: 5) {
                Image(systemName: icon)
                    .font(.system(size: 15, weight: .regular))
                Text(label)
                    .font(.caption)
            }
            .foregroundStyle(.secondary)
            .frame(minHeight: 32, alignment: .center)
            .padding(.horizontal, 8)
        }
        .buttonStyle(.borderless)
        .accessibilityLabel(label)
    }
}

// Previews omitted — KernelModel init requires the nmp_core FFI static lib
// which is not linked in the Xcode Preview host; previewing would crash.
// Test visually by running on simulator/device.
