import SwiftUI

// OWNER: Phase-2 Agent B (Thread screen).
// Type name is ThreadScreen — Bridge already defines a Decodable `ThreadView`.
// Init signature FIXED by nav contract: ThreadScreen(eventID:).

struct ThreadScreen: View {
    let eventID: String

    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    /// The note we want to present a reply compose sheet for.
    @State private var replyTarget: ChirpReplyTarget? = nil

    private var threadFeed: OpFeedSnapshot? { model.threadFeed(eventID: eventID) }
    private var cardLookup: [String: ChirpEventCard] {
        Dictionary(uniqueKeysWithValues: (threadFeed?.cards ?? []).map { ($0.card.id, $0.card) })
    }
    // ADR-0063 Lane E (#1671): the whole-map `mentionProfiles` dictionary is
    // gone. Inline mention labels read the per-key keyed-ref cache inside
    // `NoteContentView`; thread rows no longer thread a profile map.

    var body: some View {
        Group {
            if let threadFeed {
                threadContent(threadFeed)
            } else {
                loadingState
            }
        }
        .chirpScreenBackground()
        .navigationTitle("Thread")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            model.openThread(eventID: eventID)
        }
        .onDisappear {
            // T152: release the thread subscription when this view is no
            // longer visible.  Symmetric with openThread in .task above.
            model.closeThread(eventID: eventID)
        }
        .sheet(item: $replyTarget) { target in
            ComposeView(replyTo: target)
        }
    }

    // MARK: – Loading state

    private var loadingState: some View {
        VStack(spacing: 24) {
            ChirpPlaceholder(
                systemImage: "bubble.left.and.bubble.right",
                title: "Loading thread…",
                subtitle: "Notes will appear here soon."
            )
            .frame(maxHeight: 320)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: – Thread content

    @ViewBuilder
    private func threadContent(_ threadFeed: OpFeedSnapshot) -> some View {
        if threadFeed.cards.isEmpty {
            emptyThreadState
        } else {
        ScrollViewReader { proxy in
        ScrollView {
            LazyVStack(spacing: 0) {
                ForEach(threadFeed.cards) { root in
                    let card = root.card
                    let isFocused = card.id == eventID

                    ThreadNoteRow(
                        card: card,
                        isFocused: isFocused,
                        eventCards: cardLookup,
                        onAvatarTap: {
                            router.push(.profile(pubkey: card.authorPubkey))
                        },
                        onLike: {
                            model.react(targetEventID: card.id, reaction: "❤")
                        },
                        onReply: {
                            replyTarget = ChirpReplyTarget(
                                eventID: card.id,
                                authorPubkey: card.authorPubkey,
                                createdAt: card.createdAt,
                                content: card.content
                            )
                        },
                        onRepost: {
                            model.repost(eventID: card.id, authorPubkey: card.authorPubkey)
                        }
                    )
                    .id(card.id)
                    .accessibilityIdentifier(isFocused ? "thread-focused-note" : "thread-note-\(card.id.prefix(8))")

                    // Thread connector line between non-focused notes
                    if root.id != threadFeed.cards.last?.id {
                        threadConnector(isFocused: isFocused)
                    }
                }

                Spacer(minLength: 32)
            }
        }
        .accessibilityIdentifier("thread-detail-list")
        // Scroll to the focused event whenever the kernel snapshot delivers
        // a focused id — fires on first appearance (`initial: true`) and on
        // any subsequent snapshot tick that changes the focused row. This is
        // a snapshot observer, not a time-delayed sleep (AGENTS.md:60 — "No
        // polling — ever": no `DispatchQueue.main.asyncAfter` waiting for the
        // `LazyVStack` to lay out before we act). The id changing IS the
        // event we react to; SwiftUI re-runs this closure after layout has
        // resolved row identities, so `proxy.scrollTo` resolves the anchor.
        .onChange(of: eventID, initial: true) { _, newId in
            proxy.scrollTo(newId, anchor: .center)
        }
        } // ScrollViewReader
        }
    }

    // MARK: – Sub-views

    private var emptyThreadState: some View {
        ChirpPlaceholder(
            systemImage: "bubble.left.and.bubble.right",
            title: "No thread events yet",
            subtitle: "Replies will appear here when the relay returns them."
        )
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ViewBuilder
    private func threadConnector(isFocused: Bool) -> some View {
        HStack {
            // Align with avatar leading edge
            Spacer()
                .frame(width: 16 + (isFocused ? 46 : 38) / 2 - 1)
            Rectangle()
                .fill(isFocused ? ChirpColor.focusedLine : ChirpColor.hairline)
                .frame(width: 2, height: 8)
                .cornerRadius(1)
            Spacer()
        }
    }
}
