import SwiftUI

// OWNER: Phase-2 Agent B (Thread screen).
// Init signature FIXED by nav contract: ThreadScreen(eventID:).
//
// M2 Step-C (V-112, ADR-0042 §5): the thread now reads from the dynamic
// per-thread feed projection `nmp.feed.thread.<eventID>` (registered by
// `openThreadFeed`) instead of the static `thread_view` snapshot. The flat feed
// is the root by id plus every kind:1/6 referencing it via `#e`, ordered
// chronologically — it carries NO ancestor chain, focus pointer, or
// prev/next-count affordances the old `ThreadView` did. So this step renders a
// flat chronological list: the opened `eventID` is the focused row
// (`isFocused = item.id == eventID`); the "show N earlier" / "more replies"
// affordances are dropped (the flat feed has no notion of them). This is an
// intended simplification of the threaded tree for the read-migration step.

struct ThreadScreen: View {
    let eventID: String

    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    /// The event ID we want to present a reply compose sheet for.
    @State private var replyTargetID: ReplyTarget? = nil

    /// `nmp.feed.thread.<eventID>` — the dynamic per-thread feed key opened by
    /// `openThreadFeed`. Read through `model.feedProjection(key:)`.
    private var threadFeedKey: String { "nmp.feed.thread.\(eventID)" }

    /// The thread's flat feed cards (root + `#e` referrers), or `nil` until the
    /// first snapshot lands.
    private var feedCards: [ChirpRootCard]? {
        model.feedProjection(key: threadFeedKey)?.cards
    }

    /// Thread notes as synthetic `TimelineItem`s built from the feed cards (the
    /// shape `ThreadNoteRow` renders). Ordered as the kernel emits them.
    private var items: [TimelineItem] {
        (feedCards ?? []).map { TimelineItem(card: $0.card) }
    }

    private var cardLookup: [String: ChirpEventCard] {
        Dictionary(uniqueKeysWithValues: (feedCards ?? []).map { ($0.card.id, $0.card) })
    }
    private var itemLookup: [String: TimelineItem] {
        Dictionary(uniqueKeysWithValues: items.map { ($0.id, $0) })
    }

    var body: some View {
        Group {
            // `feedCards == nil` → feed not yet emitted (loading). An empty
            // (non-nil) array means the feed is open but no matching event has
            // landed yet — still show the loading affordance until the root
            // arrives so the screen never flashes an empty list.
            if let cards = feedCards, !cards.isEmpty {
                threadContent(cards)
            } else {
                loadingState
            }
        }
        .chirpScreenBackground()
        .navigationTitle("Thread")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            // M2 Step-C: open the dynamic per-thread feed (root + `#e`
            // referrers → `nmp.feed.thread.<eventID>`).
            model.openThreadFeed(eventID: eventID)
        }
        .onDisappear {
            // T152: release the thread feed when this view is no longer
            // visible. Symmetric with openThreadFeed in .task above.
            model.closeThreadFeed(eventID: eventID)
        }
        .sheet(item: $replyTargetID) { target in
            ComposeView(replyToID: target.eventID, replyToShortID: target.shortID)
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
    private func threadContent(_ cards: [ChirpRootCard]) -> some View {
        // M2 Step-C: the flat feed has no ancestor chain or "more below"
        // notion, so the "show N earlier" / "more replies" affordances are
        // gone. The opened `eventID` is the focus row.
        let rowItems = items
        ScrollViewReader { proxy in
        ScrollView {
            LazyVStack(spacing: 0) {
                // All thread items — the opened event is highlighted.
                ForEach(rowItems) { item in
                    let isFocused = item.id == eventID

                    ThreadNoteRow(
                        item: item,
                        isFocused: isFocused,
                        contentTree: cardLookup[item.id]?.contentTree,
                        mentionProfiles: model.mentionProfiles,
                        eventCards: cardLookup,
                        timelineItems: itemLookup,
                        onAvatarTap: {
                            router.push(.profile(pubkey: item.authorPubkey))
                        },
                        onLike: {
                            model.react(targetEventID: item.id, reaction: "❤")
                        },
                        onReply: {
                            // ADR-0032: shell-side abbreviation of the raw
                            // event id for ComposeView's reply banner.
                            replyTargetID = ReplyTarget(eventID: item.id, shortID: item.id.shortHex)
                        },
                        onRepost: {
                            model.repost(eventID: item.id, authorPubkey: item.authorPubkey)
                        }
                    )
                    .id(item.id)
                    .accessibilityIdentifier(isFocused ? "thread-focused-note" : "thread-note-\(item.id.prefix(8))")

                    // Thread connector line between non-focused notes
                    if item.id != rowItems.last?.id {
                        threadConnector(isFocused: isFocused)
                    }
                }

                Spacer(minLength: 32)
            }
        }
        .accessibilityIdentifier("thread-detail-list")
        // Scroll to the opened event once the feed populates it. Observing the
        // item count (not a time delay) is the snapshot event we react to
        // (AGENTS.md:60 — "No polling — ever"); SwiftUI re-runs this closure
        // after layout resolves row identities, so `proxy.scrollTo` finds the
        // anchor. `initial: true` covers the case where the root is already
        // present on first render.
        .onChange(of: rowItems.count, initial: true) { _, _ in
            guard rowItems.contains(where: { $0.id == eventID }) else { return }
            proxy.scrollTo(eventID, anchor: .center)
        }
        } // ScrollViewReader
    }

    // MARK: – Sub-views

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

// MARK: – Lightweight wrapper used for sheet(item:) presentation

private struct ReplyTarget: Identifiable {
    let eventID: String
    /// Kernel-pre-formatted abbreviation (`TimelineItem.shortId`). Forwarded
    /// to `ComposeView.replyToShortID` so the reply banner caption is bound
    /// verbatim — never sliced by Swift (V-28, aim.md §6.9).
    let shortID: String
    var id: String { eventID }
}
