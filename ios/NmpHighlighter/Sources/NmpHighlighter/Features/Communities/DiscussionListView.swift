import SwiftUI

/// Discussions tab content for a room. Uses its own `DiscussionStore`.
struct DiscussionListView: View {
    let groupId: String
    @Binding var composerPresented: Bool

    @Environment(HighlighterStore.self) private var app
    @State private var store = DiscussionStore()

    var body: some View {
        Group {
            if store.isLoading && store.discussions.isEmpty {
                ProgressView().controlSize(.large)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if store.discussions.isEmpty {
                ContentUnavailableView(
                    "No discussions yet",
                    systemImage: "bubble.left.and.bubble.right",
                    description: Text("Start one to propose a new read, ask a question, or share thoughts.")
                )
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(store.discussions, id: \.eventId) { d in
                            NavigationLink(value: d) {
                                DiscussionRow(discussion: d)
                            }
                            .buttonStyle(.plain)
                            Divider()
                                .background(Color.highlighterRule.opacity(0.4))
                                .padding(.leading, 68)
                        }
                    }
                    .padding(.horizontal, 20)
                }
                .background(Color.highlighterPaper.ignoresSafeArea())
            }
        }
        .task {
            await store.start(groupId: groupId, core: app.safeCore, bridge: app.eventBridge)
        }
        .onDisappear { store.stop() }
        .sheet(isPresented: $composerPresented) {
            DiscussionComposerView(groupId: groupId) { discussion in
                store.apply(discussion: discussion)
            }
        }
    }
}

private struct DiscussionRow: View {
    let discussion: DiscussionRecord

    @Environment(HighlighterStore.self) private var app

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            AuthorAvatar(
                pubkey: discussion.pubkey,
                pictureURL: app.profileCache[discussion.pubkey]?.picture ?? "",
                displayInitial: displayInitial,
                size: 36
            )

            VStack(alignment: .leading, spacing: 4) {
                Text(discussion.title)
                    .font(.body.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)

                if !discussion.body.isEmpty {
                    Text(discussion.body)
                        .font(.subheadline)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(3)
                        .multilineTextAlignment(.leading)
                }

                if let attachment = discussion.attachment, !attachment.title.isEmpty || !attachment.url.isEmpty {
                    attachmentChip(attachment)
                }

                HStack(spacing: 4) {
                    Text(authorName)
                        .font(.caption.weight(.medium))
                        .foregroundStyle(Color.highlighterInkMuted)
                    if let ts = discussion.createdAt, ts > 0 {
                        Text("·")
                            .font(.caption)
                            .foregroundStyle(Color.highlighterInkMuted)
                        Text(relativeTime(ts))
                            .font(.caption)
                            .foregroundStyle(Color.highlighterInkMuted)
                    }
                }
                .padding(.top, 2)
            }

            Spacer(minLength: 0)
        }
        .padding(.vertical, 14)
        .task(id: discussion.pubkey) {
            await app.requestProfile(pubkeyHex: discussion.pubkey)
        }
    }

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
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter.localizedString(for: date, relativeTo: Date())
    }

    @ViewBuilder
    private func attachmentChip(_ a: DiscussionAttachment) -> some View {
        let label = a.title.isEmpty ? a.url : a.title
        if !label.isEmpty {
            HStack(spacing: 5) {
                Image(systemName: "link")
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(Color.highlighterAccent)
                Text(label)
                    .font(.caption)
                    .foregroundStyle(Color.highlighterAccent)
                    .lineLimit(1)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(Color.highlighterAccent.opacity(0.08), in: RoundedRectangle(cornerRadius: 6, style: .continuous))
        }
    }
}
