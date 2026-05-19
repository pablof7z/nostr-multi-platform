import Kingfisher
import SwiftUI

/// Identifiable payload for `.sheet(item:)`.
///
/// Two flavours:
/// - `.artifact` / `.article` → publish a kind:11 share of the underlying
///   article/book/podcast into the target room (existing flow). Uses the
///   `preview` payload to construct the kind:11 event.
/// - `.highlight` → publish a kind:16 generic repost referencing the
///   selected highlight directly. The repost carries `["e", id]`,
///   `["k", "9802"]`, `["p", author]`, `["h", target_group_id]`.
struct ShareToCommunityTarget: Identifiable {
    let id = UUID()
    let kind: Kind
    let displayTitle: String
    let displaySubtitle: String
    let imageURL: URL?

    enum Kind {
        /// Share the source artifact/article via kind:11.
        case artifactShare(preview: ArtifactPreview)
        /// Re-share an existing highlight via kind:16.
        case highlightRepost(eventId: String, authorPubkeyHex: String, relayHint: String)
    }

    static func article(_ article: ArticleRecord) -> ShareToCommunityTarget {
        let preview = ArtifactPreviewBuilder.from(article: article)
        return ShareToCommunityTarget(
            kind: .artifactShare(preview: preview),
            displayTitle: article.title.isEmpty ? "Untitled" : article.title,
            displaySubtitle: article.summary,
            imageURL: article.image.isEmpty ? nil : URL(string: article.image)
        )
    }

    static func artifact(_ artifact: ArtifactRecord) -> ShareToCommunityTarget {
        let preview = ArtifactPreviewBuilder.from(artifact: artifact)
        return ShareToCommunityTarget(
            kind: .artifactShare(preview: preview),
            displayTitle: artifact.preview.title.isEmpty ? "Untitled" : artifact.preview.title,
            displaySubtitle: artifact.preview.description,
            imageURL: artifact.preview.image.isEmpty ? nil : URL(string: artifact.preview.image)
        )
    }

    /// Share the highlight quote itself (not the source artifact). The
    /// repost references the kind:9802 highlight event by id, so anyone
    /// in the room sees the friend's quote with full attribution.
    static func highlight(
        _ highlight: HighlightRecord,
        relayHint: String = ""
    ) -> ShareToCommunityTarget {
        let snippet = highlight.quote.isEmpty
            ? "Highlight"
            : "\u{201C}\(highlight.quote)\u{201D}"
        return ShareToCommunityTarget(
            kind: .highlightRepost(
                eventId: highlight.eventId,
                authorPubkeyHex: highlight.pubkey,
                relayHint: relayHint
            ),
            displayTitle: snippet,
            displaySubtitle: highlight.note,
            imageURL: nil
        )
    }
}

/// Sheet that lets the user pick which community to publish an article / re-share
/// to, with an optional note.
struct ShareToCommunitySheet: View {
    @Environment(HighlighterStore.self) private var app
    @Environment(\.dismiss) private var dismiss

    let target: ShareToCommunityTarget

    @State private var note: String = ""
    @State private var publishingId: String?
    @State private var errorMessage: String?

    var body: some View {
        NavigationStack {
            List {
                Section {
                    headerCard
                        .listRowInsets(EdgeInsets(top: 12, leading: 16, bottom: 12, trailing: 16))
                }

                Section("Note (optional)") {
                    TextField("What caught your attention?", text: $note, axis: .vertical)
                        .lineLimit(2...6)
                }

                Section("Share to") {
                    if app.joinedCommunities.isEmpty {
                        Text("You haven't joined any communities yet.")
                            .foregroundStyle(Color.highlighterInkMuted)
                    } else {
                        ForEach(app.joinedCommunities, id: \.id) { community in
                            Button {
                                publish(to: community.id)
                            } label: {
                                communityRow(community)
                            }
                            .disabled(publishingId != nil)
                        }
                    }
                }
            }
            .navigationTitle(navigationTitle)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .disabled(publishingId != nil)
                }
            }
            .alert("Couldn't share", isPresented: Binding(
                get: { errorMessage != nil },
                set: { if !$0 { errorMessage = nil } }
            )) {
                Button("OK", role: .cancel) { errorMessage = nil }
            } message: {
                Text(errorMessage ?? "")
            }
        }
    }

    // MARK: - Header card

    private var headerCard: some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 6) {
                Text(target.displayTitle)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(3)
                if !target.displaySubtitle.isEmpty {
                    Text(target.displaySubtitle)
                        .font(.caption)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(2)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            if let url = target.imageURL {
                KFImage(url)
                    .placeholder { Color.highlighterRule.opacity(0.4) }
                    .fade(duration: 0.15)
                    .resizable()
                    .scaledToFill()
                    .frame(width: 64, height: 64)
                    .clipShape(RoundedRectangle(cornerRadius: 8))
            }
        }
    }

    // MARK: - Community row

    private func communityRow(_ community: CommunitySummary) -> some View {
        HStack(spacing: 12) {
            if let url = URL(string: community.picture), !community.picture.isEmpty {
                KFImage(url)
                    .placeholder { Color.highlighterRule.opacity(0.4) }
                    .fade(duration: 0.15)
                    .resizable()
                    .scaledToFill()
                    .frame(width: 32, height: 32)
                    .clipShape(RoundedRectangle(cornerRadius: 6))
            } else {
                Image(systemName: "square.grid.2x2")
                    .frame(width: 32, height: 32)
                    .foregroundStyle(Color.highlighterInkMuted)
            }

            Text(community.name.isEmpty ? community.id : community.name)
                .foregroundStyle(Color.highlighterInkStrong)

            Spacer()

            if publishingId == community.id {
                ProgressView()
            }
        }
    }

    // MARK: - Action

    private var navigationTitle: String {
        switch target.kind {
        case .artifactShare: return "Share to community"
        case .highlightRepost: return "Share highlight"
        }
    }

    private func publish(to groupId: String) {
        guard publishingId == nil else { return }
        publishingId = groupId
        let trimmedNote = note.trimmingCharacters(in: .whitespacesAndNewlines)
        Task {
            do {
                switch target.kind {
                case .artifactShare(let preview):
                    _ = try await app.safeCore.publishArtifact(
                        preview: preview,
                        groupId: groupId,
                        note: trimmedNote.isEmpty ? nil : trimmedNote
                    )
                case .highlightRepost(let eventId, let authorPubkey, let relayHint):
                    try await app.safeCore.shareHighlightToRoom(
                        highlightId: eventId,
                        highlightAuthorPubkeyHex: authorPubkey,
                        highlightRelayUrl: relayHint,
                        targetGroupId: groupId
                    )
                }
                await MainActor.run {
                    UINotificationFeedbackGenerator().notificationOccurred(.success)
                    dismiss()
                }
            } catch {
                await MainActor.run {
                    publishingId = nil
                    errorMessage = error.localizedDescription
                }
            }
        }
    }
}
