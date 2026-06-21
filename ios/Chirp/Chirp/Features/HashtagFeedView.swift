import SwiftUI

// Hashtag feed — a flat list of kind:1 notes carrying a NIP-12 `#t` tag.
//
// Thin-shell rule: no business logic in Swift. Rust owns the tag-feed
// membership under `nmp.feed.tag.<tag>` (Chirp declares primary kind `[1]`;
// the NIP-18 adapter derives repost-wrapper acquisition). This view only opens
// the feed on appear, releases it on disappear, and renders the same flat-feed
// projection the profile/thread screens use.
struct HashtagFeedView: View {
    /// Normalized tag (lowercased, no leading `#`) — the classifier produced it.
    let tag: String

    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    private var items: [ChirpRootCard] { model.tagFeed(tag: tag)?.cards ?? [] }

    /// Render context fed to each `ProfileNoteRow` — identical construction to
    /// `ProfileView` (the Rust-derived `mentionProfiles` plus a per-pass card
    /// lookup); the shell never resolves display names itself.
    private var noteRenderContext: NoteRenderContext {
        NoteRenderContext(
            mentionProfiles: model.mentionProfiles,
            eventCards: Dictionary(uniqueKeysWithValues: items.map { ($0.card.id, $0.card) }),
            timelineItems: [:]
        )
    }

    var body: some View {
        ScrollView {
            notesSection
        }
        .accessibilityIdentifier("hashtag-feed")
        .chirpScreenBackground()
        .navigationTitle("#\(tag)")
        .navigationBarTitleDisplayMode(.inline)
        .task { model.openTag(tag: tag) }
        .onDisappear { model.closeTag(tag: tag) }
        .animation(.smooth(duration: 0.25), value: items.count)
    }

    @ViewBuilder
    private var notesSection: some View {
        if items.isEmpty {
            ChirpPlaceholder(
                systemImage: "number",
                title: "No posts yet",
                subtitle: "Posts tagged #\(tag) will appear here."
            )
            .frame(minHeight: 320)
        } else {
            let context = noteRenderContext
            LazyVStack(spacing: 0) {
                ForEach(items) { root in
                    let card = root.card
                    ProfileNoteRow(
                        card: card,
                        renderContext: context,
                        onAvatarTap: {
                            router.push(.profile(pubkey: card.authorPubkey))
                        },
                        onRowTap: {
                            router.push(.thread(eventID: card.id))
                        },
                        onLike: {
                            model.react(targetEventID: card.id, reaction: "❤")
                        },
                        onRepost: {
                            model.repost(eventID: card.id, authorPubkey: card.authorPubkey)
                        }
                    )

                    if root.id != items.last?.id {
                        Divider()
                            .padding(.leading, 68)
                    }
                }
            }
        }
    }
}
