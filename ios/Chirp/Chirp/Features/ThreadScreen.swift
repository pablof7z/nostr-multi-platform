import SwiftUI

// OWNER: Phase-2 Agent B (Thread screen).
// Type name is ThreadScreen — Bridge already defines a Decodable `ThreadView`.
// Init signature FIXED by nav contract: ThreadScreen(eventID:).

struct ThreadScreen: View {
    let eventID: String

    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    /// The event ID we want to present a reply compose sheet for.
    @State private var replyTargetID: ReplyTarget? = nil

    private var thread: ThreadView? { model.threadView }
    private var cardLookup: [String: ChirpEventCard] {
        // V-80 — the home feed is now roots-only (`cards: [ChirpRootCard]`);
        // reach into each root's inner `.card`. This is a best-effort side
        // lookup — the thread view's primary card source is `thread?.items`.
        Dictionary(uniqueKeysWithValues: model.modularTimeline.cards.map { ($0.card.id, $0.card) })
    }
    private var itemLookup: [String: TimelineItem] {
        Dictionary(uniqueKeysWithValues: (thread?.items ?? model.items).map { ($0.id, $0) })
    }
    // V-31 — `mention_profiles` snapshot projection now covers thread-view
    // items (see `update.rs` `mention_profiles` block), so the Swift
    // `Dictionary(items.map …)` derivation this view used to build is gone.
    // Bind `model.mentionProfiles` directly at the call site.

    var body: some View {
        Group {
            if let thread {
                threadContent(thread)
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
    private func threadContent(_ thread: ThreadView) -> some View {
        ScrollViewReader { proxy in
        ScrollView {
            LazyVStack(spacing: 0) {
                // "Show N earlier" affordance — kernel pre-formats the label
                // (aim.md §6 anti-pattern #1). `??` falls back to empty for
                // older kernel builds; the `previousCount > 0` gate suppresses
                // an empty row in that case.
                if thread.previousCount > 0 {
                    earlierAffordance(label: thread.previousCountLabel ?? "")
                }

                // All thread items — focused one is highlighted
                ForEach(thread.items) { item in
                    let isFocused = item.id == thread.focusedEventId

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
                        }
                    )
                    .id(item.id)
                    .accessibilityIdentifier(isFocused ? "thread-focused-note" : "thread-note-\(item.id.prefix(8))")

                    // Thread connector line between non-focused notes
                    if item.id != thread.items.last?.id {
                        threadConnector(isFocused: isFocused)
                    }
                }

                // More replies below affordance. Kernel pre-formats the
                // pluralized label (aim.md §6 anti-pattern #1: no native-side
                // pluralization). `??` falls back to the empty string for
                // older kernel builds that predate `nextCountLabel` — the row
                // collapses naturally because `nextCount > 0` is the gate.
                if thread.nextCount > 0 {
                    HStack(spacing: 4) {
                        Image(systemName: "ellipsis.bubble")
                            .font(.system(size: 13))
                        Text(thread.nextCountLabel ?? "")
                            .font(.callout)
                    }
                    .foregroundStyle(.secondary)
                    .padding(.vertical, 8)
                    .padding(.horizontal, 16)
                    .frame(maxWidth: .infinity, alignment: .leading)
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
        .onChange(of: thread.focusedEventId, initial: true) { _, newId in
            proxy.scrollTo(newId, anchor: .center)
        }
        } // ScrollViewReader
    }

    // MARK: – Sub-views

    private func earlierAffordance(label: String) -> some View {
        HStack(spacing: 4) {
            Image(systemName: "arrow.up.circle")
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(ChirpColor.link)
            Text(label)
                .font(.callout)
                .foregroundStyle(ChirpColor.link)
            Spacer()
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .overlay(alignment: .bottom) {
            Divider()
        }
        .contentShape(Rectangle())
        .onTapGesture {
            // No kernel command exists to expand context yet — haptic feedback only.
            let g = UIImpactFeedbackGenerator(style: .light)
            g.impactOccurred()
        }
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

// MARK: – Lightweight wrapper used for sheet(item:) presentation

private struct ReplyTarget: Identifiable {
    let eventID: String
    /// Kernel-pre-formatted abbreviation (`TimelineItem.shortId`). Forwarded
    /// to `ComposeView.replyToShortID` so the reply banner caption is bound
    /// verbatim — never sliced by Swift (V-28, aim.md §6.9).
    let shortID: String
    var id: String { eventID }
}
