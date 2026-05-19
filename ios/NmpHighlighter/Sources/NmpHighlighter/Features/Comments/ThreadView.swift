import SwiftUI

/// Root and pushed destination for NIP-22 thread navigation.
///
/// At root (`focused == nil`) renders the artifact's top-level comments.
/// When pushed (`focused != nil`) renders that comment's direct children
/// as its heading and lists its replies. Recursive drill-down is handled
/// by the local `focusedNode` state + `navigationDestination(item:)` so
/// every level lives in the enclosing NavigationStack — no nested stacks.
struct ThreadView: View {
    let focused: CommentNode?
    let artifactHeader: AnyView?
    let store: CommentsStore
    let artifact: ArtifactRef
    let artifactAuthorPubkey: String?

    @Environment(\.dismiss) private var dismiss
    @State private var focusedNode: CommentNode? = nil

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    if let focused {
                        focusedHeader(focused)
                            .padding(.bottom, 4)
                    } else if let artifactHeader {
                        artifactHeader
                            .padding(.bottom, 4)
                    }

                    if children.isEmpty {
                        emptyState
                    } else {
                        ForEach(children) { child in
                            VStack(spacing: 0) {
                                CommentRow(
                                    node: child,
                                    depth: 0,
                                    isAuthorReply: false,
                                    onTap: { focusOn(child) },
                                    store: store
                                )
                                inlineReplyPreview(for: child)
                                Divider()
                                    .background(Color.highlighterRule.opacity(0.4))
                            }
                        }
                    }
                }
            }
            .scrollDismissesKeyboard(.interactively)

            CommentComposer(
                parentEventId: focused?.record.eventId,
                placeholder: composerPlaceholder,
                store: store
            )
        }
        .background(Color.highlighterPaper.ignoresSafeArea())
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .principal) {
                Text(navTitle)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(Color.highlighterInkStrong)
            }
        }
        .navigationDestination(item: $focusedNode) { node in
            ThreadView(
                focused: Self.locate(eventId: node.record.eventId, in: store.tree) ?? node,
                artifactHeader: nil,
                store: store,
                artifact: artifact,
                artifactAuthorPubkey: artifactAuthorPubkey
            )
        }
    }

    // MARK: - Children resolution

    private var children: [CommentNode] {
        if let focused {
            return Self.locate(eventId: focused.record.eventId, in: store.tree)?.children
                ?? focused.children
        }
        return store.tree
    }

    static func locate(eventId: String, in nodes: [CommentNode]) -> CommentNode? {
        for n in nodes {
            if n.record.eventId == eventId { return n }
            if let hit = locate(eventId: eventId, in: n.children) { return hit }
        }
        return nil
    }

    // MARK: - Inline reply preview

    @ViewBuilder
    private func inlineReplyPreview(for parent: CommentNode) -> some View {
        if let mostRecent = parent.mostRecentReply {
            let isAuthorReply = (artifactAuthorPubkey != nil)
                && (mostRecent.record.pubkey == artifactAuthorPubkey)
            CommentRow(
                node: mostRecent,
                depth: 1,
                isAuthorReply: isAuthorReply,
                onTap: { focusOn(mostRecent) },
                store: store
            )
            .padding(.leading, 18)
            .padding(.trailing, 18)

            if parent.children.count > 1 {
                moreRepliesChip(parent: parent)
            }
        }
    }

    private func moreRepliesChip(parent: CommentNode) -> some View {
        Button {
            focusOn(parent)
        } label: {
            HStack(spacing: 6) {
                Spacer()
                    .frame(width: 36 + 18 + 12, alignment: .leading)
                Text("View \(parent.children.count - 1) more \(parent.children.count - 1 == 1 ? "reply" : "replies")")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(Color.highlighterAccent)
                Image(systemName: "chevron.right")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(Color.highlighterAccent)
                Spacer()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.vertical, 6)
        }
        .buttonStyle(.plain)
    }

    // MARK: - Focused-comment header

    @ViewBuilder
    private func focusedHeader(_ node: CommentNode) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            CommentRow(
                node: node,
                depth: 0,
                isAuthorReply: false,
                onTap: {},
                store: store
            )
            .allowsHitTesting(false)
            HStack(spacing: 6) {
                Image(systemName: "arrow.turn.down.right")
                    .font(.caption)
                    .foregroundStyle(Color.highlighterInkMuted)
                Text(replyCountLabel(for: node))
                    .font(.caption.weight(.medium))
                    .foregroundStyle(Color.highlighterInkMuted)
                    .textCase(.uppercase)
                    .tracking(0.6)
                Spacer()
            }
            .padding(.horizontal, 18)
            .padding(.bottom, 6)
            Rectangle()
                .fill(Color.highlighterRule.opacity(0.4))
                .frame(height: 0.5)
        }
    }

    private func replyCountLabel(for node: CommentNode) -> String {
        let count = (Self.locate(eventId: node.record.eventId, in: store.tree)?.children.count)
            ?? node.children.count
        if count == 0 { return "Be the first to reply" }
        if count == 1 { return "1 reply" }
        return "\(count) replies"
    }

    // MARK: - Empty state

    private var emptyState: some View {
        VStack(spacing: 8) {
            Image(systemName: "bubble.left.and.bubble.right")
                .font(.system(size: 28, weight: .light))
                .foregroundStyle(Color.highlighterInkMuted)
            Text(emptyStateLabel)
                .font(.subheadline)
                .foregroundStyle(Color.highlighterInkMuted)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 60)
    }

    private var emptyStateLabel: String {
        focused == nil ? "Start the conversation." : "Be the first to reply."
    }

    // MARK: - Helpers

    private var navTitle: String {
        if focused != nil { return "Reply thread" }
        let count = store.totalCount
        if count == 0 { return "Comments" }
        if count == 1 { return "1 comment" }
        return "\(count) comments"
    }

    private var composerPlaceholder: String {
        focused == nil ? "Add to the conversation" : "Reply…"
    }

    private func focusOn(_ node: CommentNode) {
        focusedNode = node
    }
}
