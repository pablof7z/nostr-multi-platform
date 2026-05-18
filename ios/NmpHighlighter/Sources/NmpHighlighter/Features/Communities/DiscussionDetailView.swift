import SwiftUI

struct DiscussionDetailView: View {
    let discussion: DiscussionRecord

    @Environment(HighlighterStore.self) private var app
    @State private var store = CommentsStore()
    @State private var focusedNode: CommentNode? = nil

    private var artifactRef: ArtifactRef { .event(id: discussion.eventId, kind: 11) }

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    opHeader
                        .padding(.horizontal, 18)
                        .padding(.vertical, 16)

                    Rectangle()
                        .fill(Color.highlighterRule.opacity(0.5))
                        .frame(height: 0.5)

                    repliesSection
                }
            }
            .scrollDismissesKeyboard(.interactively)

            CommentComposer(
                parentEventId: nil,
                placeholder: "Add to the conversation",
                store: store
            )
        }
        .background(Color.highlighterPaper.ignoresSafeArea())
        .navigationTitle(discussion.title)
        .navigationBarTitleDisplayMode(.inline)
        .task {
            await store.start(
                artifact: artifactRef,
                core: app.safeCore,
                currentUserPubkey: app.currentUser?.pubkey
            )
        }
        .navigationDestination(item: $focusedNode) { node in
            ThreadView(
                focused: ThreadView.locate(eventId: node.record.eventId, in: store.tree) ?? node,
                artifactHeader: nil,
                store: store,
                artifact: artifactRef,
                artifactAuthorPubkey: discussion.pubkey
            )
        }
    }

    // MARK: - OP header

    private var opHeader: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 10) {
                AuthorAvatar(
                    pubkey: discussion.pubkey,
                    pictureURL: app.profileCache[discussion.pubkey]?.picture ?? "",
                    displayInitial: displayInitial,
                    size: 38
                )

                VStack(alignment: .leading, spacing: 2) {
                    Text(authorName)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(Color.highlighterInkStrong)
                    if let ts = discussion.createdAt, ts > 0 {
                        Text(relativeTime(ts))
                            .font(.caption)
                            .foregroundStyle(Color.highlighterInkMuted)
                    }
                }
            }

            Text(discussion.title)
                .font(.title3.weight(.bold))
                .foregroundStyle(Color.highlighterInkStrong)
                .fixedSize(horizontal: false, vertical: true)

            if !discussion.body.isEmpty {
                Text(discussion.body)
                    .font(.body)
                    .foregroundStyle(Color.highlighterInkStrong)
                    .fixedSize(horizontal: false, vertical: true)
                    .lineSpacing(3)
            }

            if let attachment = discussion.attachment {
                attachmentCard(attachment)
            }
        }
        .task(id: discussion.pubkey) {
            await app.requestProfile(pubkeyHex: discussion.pubkey)
        }
    }

    @ViewBuilder
    private func attachmentCard(_ a: DiscussionAttachment) -> some View {
        let title = a.title.isEmpty ? a.url : a.title
        if !title.isEmpty {
            HStack(spacing: 10) {
                if !a.image.isEmpty, let url = URL(string: a.image) {
                    AsyncImage(url: url) { phase in
                        if let img = phase.image {
                            img.resizable().scaledToFill()
                        } else {
                            Color.highlighterInkMuted.opacity(0.12)
                        }
                    }
                    .frame(width: 52, height: 52)
                    .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                } else {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .fill(Color.highlighterAccent.opacity(0.1))
                        .frame(width: 52, height: 52)
                        .overlay {
                            Image(systemName: "link")
                                .font(.system(size: 18, weight: .medium))
                                .foregroundStyle(Color.highlighterAccent)
                        }
                }

                VStack(alignment: .leading, spacing: 3) {
                    Text(title)
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(Color.highlighterInkStrong)
                        .lineLimit(2)
                    if !a.author.isEmpty {
                        Text(a.author)
                            .font(.caption)
                            .foregroundStyle(Color.highlighterInkMuted)
                            .lineLimit(1)
                    }
                }

                Spacer(minLength: 0)
            }
            .padding(10)
            .background(
                Color.highlighterInkStrong.opacity(0.04),
                in: RoundedRectangle(cornerRadius: 10, style: .continuous)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(Color.highlighterRule, lineWidth: 0.5)
            )
        }
    }

    // MARK: - Replies section

    @ViewBuilder
    private var repliesSection: some View {
        if store.isLoading && store.tree.isEmpty {
            ProgressView()
                .frame(maxWidth: .infinity)
                .padding(.vertical, 40)
        } else if store.tree.isEmpty {
            VStack(spacing: 8) {
                Image(systemName: "bubble.left.and.bubble.right")
                    .font(.system(size: 28, weight: .light))
                    .foregroundStyle(Color.highlighterInkMuted)
                Text("Start the conversation.")
                    .font(.subheadline)
                    .foregroundStyle(Color.highlighterInkMuted)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 48)
        } else {
            ForEach(store.tree) { node in
                VStack(spacing: 0) {
                    CommentRow(
                        node: node,
                        depth: 0,
                        isAuthorReply: node.record.pubkey == discussion.pubkey,
                        onTap: { focusedNode = node },
                        store: store
                    )
                    inlineReplyPreview(for: node)
                    Divider()
                        .background(Color.highlighterRule.opacity(0.4))
                }
            }
        }
    }

    @ViewBuilder
    private func inlineReplyPreview(for parent: CommentNode) -> some View {
        if let mostRecent = parent.mostRecentReply {
            CommentRow(
                node: mostRecent,
                depth: 1,
                isAuthorReply: mostRecent.record.pubkey == discussion.pubkey,
                onTap: { focusedNode = mostRecent },
                store: store
            )
            .padding(.leading, 18)
            .padding(.trailing, 18)

            if parent.children.count > 1 {
                Button { focusedNode = parent } label: {
                    HStack(spacing: 6) {
                        Spacer().frame(width: 36 + 18 + 12)
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
        }
    }

    // MARK: - Helpers

    private var authorName: String {
        let profile = app.profileCache[discussion.pubkey]
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        return String(discussion.pubkey.prefix(8))
    }

    private var displayInitial: String {
        let profile = app.profileCache[discussion.pubkey]
        let name = profile?.displayName ?? profile?.name ?? ""
        return name.first.map { String($0).uppercased() } ?? String(discussion.pubkey.prefix(1).uppercased())
    }

    private func relativeTime(_ timestamp: UInt64) -> String {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter.localizedString(for: Date(timeIntervalSince1970: TimeInterval(timestamp)), relativeTo: Date())
    }
}
