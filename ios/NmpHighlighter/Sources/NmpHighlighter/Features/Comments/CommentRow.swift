import SwiftUI

/// Whisper-quiet cell. The whole row is a tap target; the parent owns
/// the push semantics (`onTap`). Long-press surfaces the action menu
/// (Like, Bookmark, Copy, …) via the system contextMenu.
///
/// Renders top-level (depth 0) at full size; depth 1 is rendered with a
/// smaller avatar and an indented thread line. Past depth 1, the parent
/// thread view delegates to a pushed thread instead of nesting visually.
struct CommentRow: View {
    let node: CommentNode
    /// 0 = top-level, 1 = inline reply preview. The row itself never
    /// renders deeper indents — recursion happens via thread push.
    let depth: Int
    /// Tints the thread line gold when this reply is by the artifact's
    /// own author (article author, podcaster, …).
    let isAuthorReply: Bool
    let onTap: () -> Void

    let store: CommentsStore

    @Environment(HighlighterStore.self) private var app

    var body: some View {
        Button(action: onTap) {
            HStack(alignment: .top, spacing: 0) {
                if depth > 0 {
                    threadRail
                        .padding(.trailing, 10)
                }

                HStack(alignment: .top, spacing: 12) {
                    AuthorAvatar(
                        pubkey: node.record.pubkey,
                        pictureURL: app.profileCache[node.record.pubkey]?.picture ?? "",
                        displayInitial: initial(for: node.record.pubkey),
                        size: depth == 0 ? 40 : 30,
                        ringWidth: 1.5
                    )

                    VStack(alignment: .leading, spacing: 6) {
                        headerLine
                        NostrRichText(
                            content: node.record.body,
                            font: depth == 0 ? .body : .subheadline,
                            ink: Color.highlighterInkStrong
                        )
                        .multilineTextAlignment(.leading)
                        .fixedSize(horizontal: false, vertical: true)
                        footer
                    }
                    Spacer(minLength: 0)
                }
            }
            .padding(.horizontal, depth == 0 ? 18 : 0)
            .padding(.vertical, 10)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .contextMenu {
            actionMenu
        }
        .task(id: node.record.pubkey) {
            await app.requestProfile(pubkeyHex: node.record.pubkey)
        }
    }

    // MARK: - Header line (name · time · trailing reply chevron)

    @ViewBuilder
    private var headerLine: some View {
        HStack(spacing: 6) {
            Text(displayName)
                .font(.system(size: depth == 0 ? 15 : 13, weight: .semibold))
                .foregroundStyle(Color.highlighterInkStrong)
                .lineLimit(1)
            if let rel = relativeTime {
                Text("·").foregroundStyle(Color.highlighterInkMuted)
                Text(rel)
                    .font(.system(size: depth == 0 ? 13 : 12))
                    .foregroundStyle(Color.highlighterInkMuted)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
            if !node.children.isEmpty {
                replyChevron
            }
        }
    }

    private var replyChevron: some View {
        HStack(spacing: 2) {
            Text("\(node.children.count)")
                .font(.system(size: 12, weight: .medium, design: .rounded))
                .foregroundStyle(Color.highlighterInkMuted)
                .monospacedDigit()
            Image(systemName: "chevron.right")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(Color.highlighterInkMuted)
        }
    }

    // MARK: - Footer (heart + count)

    @ViewBuilder
    private var footer: some View {
        let liked = store.isLiked(node.record.eventId)
        let count = store.likeCount(node.record.eventId)
        if liked || count > 0 {
            HStack(spacing: 6) {
                Image(systemName: liked ? "heart.fill" : "heart")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(liked ? Color.highlighterAccent : Color.highlighterInkMuted)
                if count > 0 {
                    Text("\(count)")
                        .font(.system(size: 12, weight: .medium, design: .rounded))
                        .foregroundStyle(Color.highlighterInkMuted)
                        .monospacedDigit()
                }
                Spacer(minLength: 0)
            }
            .padding(.top, 2)
            .opacity(liked ? 1.0 : 0.65)
        }
    }

    // MARK: - Thread rail (inline reply only)

    private var threadRail: some View {
        Rectangle()
            .fill(
                isAuthorReply
                    ? Color.highlighterAccent
                    : Color.highlighterAccent.opacity(0.30)
            )
            .frame(width: 2)
            .frame(maxHeight: .infinity)
            .padding(.leading, 36)
    }

    // MARK: - Long-press menu

    @ViewBuilder
    private var actionMenu: some View {
        let isBookmarked = store.isBookmarked(node.record.eventId)
        Button {
            Task { await store.toggleLike(node.record) }
        } label: {
            Label(
                store.isLiked(node.record.eventId) ? "Unlike" : "Like",
                systemImage: store.isLiked(node.record.eventId) ? "heart.slash" : "heart"
            )
        }
        Button {
            Task { await store.toggleBookmark(node.record) }
        } label: {
            Label(
                isBookmarked ? "Remove bookmark" : "Bookmark",
                systemImage: isBookmarked ? "bookmark.slash" : "bookmark"
            )
        }
        Button {
            UIPasteboard.general.string = node.record.body
        } label: {
            Label("Copy text", systemImage: "doc.on.doc")
        }
        // Profile / mute hooks stub — no plumbing for v1.
    }

    // MARK: - Helpers

    private var displayName: String {
        let profile = app.profileCache[node.record.pubkey]
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        return String(node.record.pubkey.prefix(10))
    }

    private func initial(for pubkey: String) -> String {
        displayName.first.map { String($0).uppercased() } ?? ""
    }

    private var relativeTime: String? {
        guard let s = node.record.createdAt, s > 0 else { return nil }
        let date = Date(timeIntervalSince1970: TimeInterval(s))
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        formatter.dateTimeStyle = .numeric
        return formatter.localizedString(for: date, relativeTo: Date())
    }
}
