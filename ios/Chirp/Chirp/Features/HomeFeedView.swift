import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// HomeFeedView — Home timeline root for Chirp.
//
// Renders `model.modularTimeline.blocks` (T146) using `ModularBlockView`:
// `Standalone` blocks delegate to the existing `NoteRowView`; `Module`
// blocks stack two-or-three events vertically with a connecting line in
// the avatar column. The flat `model.items` list is still around and is
// consumed by `ProfileView` / `ThreadScreen` (M2 follow-up migrates them).
//
// Empty state and pull-to-refresh stay unchanged. The blocks/cards lookup
// table is rebuilt every body pass — `[TimelineBlock]` and
// `[ChirpEventCard]` are small (≤ visible_limit; ≤80 by default), so the
// renderer doesn't need to memoize it.
// ─────────────────────────────────────────────────────────────────────────

struct HomeFeedView: View {
    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    /// Controls the top-level "new note" compose sheet (toolbar button).
    @State private var showCompose = false
    /// Controls the publish outbox sheet.
    @State private var showOutbox = false
    /// Last page edge that triggered an older-window request.
    @State private var lastLoadMoreCursor: TimelineWindowCursor?

    var body: some View {
        ZStack {
            if isEmpty {
                emptyState
            } else {
                timeline
            }
        }
        .accessibilityIdentifier("home-feed")
        .chirpScreenBackground()
        .navigationTitle("Chirp")
        .navigationBarTitleDisplayMode(.large)
        .toolbar { toolbarContent }
        .task { model.openTimeline() }
        .sheet(isPresented: $showCompose) {
            ComposeView()
        }
        .sheet(isPresented: $showOutbox) {
            NavigationStack {
                NotificationsView()
            }
        }
    }

    // T146 — empty when neither blocks nor the legacy flat list has
    // anything to render. The legacy fallback is the safety net for any
    // surface where the projection hasn't caught up yet (e.g. cold boot
    // before the first observer fan-out reaches Swift).
    private var isEmpty: Bool {
        model.modularTimeline.blocks.isEmpty && model.items.isEmpty
    }

    private var currentAccount: AccountSummary? {
        guard let activeID = model.activeAccount else { return nil }
        return model.accounts.first { $0.id == activeID }
    }

    // ── Timeline list ──────────────────────────────────────────────────────

    private var timeline: some View {
        TimelineListView(
            blocks: effectiveBlocks,
            cards: model.modularTimeline.cards,
            nextCursor: model.modularTimeline.page?.nextCursor,
            items: model.items,
            // V-31 — kernel-owned `mention_profiles` projection covers every
            // home-timeline author (and any open author/thread view),
            // replacing the Swift Dictionary derivation `TimelineListView`
            // used to build from `items.map(...)` (D4: derived view from
            // kernel, not reconstructed by shell).
            mentionProfiles: model.mentionProfiles,
            onRefresh: { model.openTimeline() },
            onLike: { model.react(targetEventID: $0, reaction: "❤") },
            // NIP-57 — 21 sats default until an amount picker lands.
            // `lnurl` is the pre-extracted `authorLnurl` from the timeline
            // item (Rust decides zapability; the row only surfaces this
            // closure when the field is non-nil — see `NoteActionsRow`).
            onZap: { eventID, pubkey, lnurl in
                model.zap(targetEventID: eventID, authorPubkey: pubkey, lnurl: lnurl)
            },
            onLoadMore: { cursor in
                // Rust clears `nextCursor` once no older page can be served,
                // including at the window cap, so this cannot retry-spin.
                guard lastLoadMoreCursor != cursor else { return }
                lastLoadMoreCursor = cursor
                model.loadOlderTimeline()
            }
        )
        .equatable()
    }

    // T146 — render modular blocks if any have been projected; otherwise
    // synthesize one Standalone block per `TimelineItem` so the cold-boot
    // window (before the projection has accepted its first event) still
    // shows the kernel's flat list. This is the only Swift-side fallback
    // in the modular-vs-flat split.
    private var effectiveBlocks: [TimelineBlock] {
        if !model.modularTimeline.blocks.isEmpty {
            return model.modularTimeline.blocks
        }
        return model.items.map { .standalone(eventID: $0.id) }
    }

    // ── Empty / loading state ─────────────────────────────────────────────

    private var emptyState: some View {
        ScrollView {
            ChirpPlaceholder(
                systemImage: "bird",
                title: "Your timeline",
                subtitle: "Loading your timeline…"
            )
            .frame(minHeight: 500)
            .padding(.horizontal, ChirpSpace.l)
        }
        .scrollContentBackground(.hidden)
        .refreshable {
            model.openTimeline()
        }
    }

    // ── Toolbar: compose + activity ───────────────────────────────────────

    @ToolbarContentBuilder
    private var toolbarContent: some ToolbarContent {
        ToolbarItem(placement: .navigationBarLeading) {
            if let account = currentAccount {
                Button {
                    router.push(.profile(pubkey: account.id))
                } label: {
                    ChirpAvatar(
                        url: account.pictureUrl,
                        initials: (account.displayName ?? account.id).displayInitials,
                        colorHex: account.id.pubkeyColorHex,
                        size: 32
                    )
                    .frame(width: 44, height: 44)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Open your profile")
            }
        }

        ToolbarItem(placement: .navigationBarTrailing) {
            Button {
                showOutbox = true
            } label: {
                ZStack(alignment: .topTrailing) {
                    Image(systemName: "paperplane")
                        .font(.system(size: 17, weight: .semibold))
                    if !model.publishOutbox.isEmpty {
                        Text("\(min(model.publishOutbox.count, 9))")
                            .font(.system(size: 9, weight: .bold))
                            .foregroundStyle(ChirpColor.emphasisForeground)
                            .frame(minWidth: 14, minHeight: 14)
                            .background(ChirpColor.accent, in: Circle())
                            .offset(x: 8, y: -8)
                    }
                }
            }
            .accessibilityLabel("Publish outbox")
            .accessibilityIdentifier("publish-outbox-button")
        }

        ToolbarItem(placement: .navigationBarTrailing) {
            Button {
                showCompose = true
            } label: {
                Image(systemName: "square.and.pencil")
            }
            .accessibilityLabel("New note")
        }
    }
}

private struct TimelineListView: View, Equatable {
    let blocks: [TimelineBlock]
    let cards: [ChirpEventCard]
    let nextCursor: TimelineWindowCursor?
    let items: [TimelineItem]
    /// V-31 — kernel-owned mention-profile map (replaces the Swift
    /// `Dictionary(items.map …)` derivation this view used to build). Bound
    /// from `model.mentionProfiles`, which reads the `mention_profiles`
    /// snapshot projection (`update.rs` covers home-timeline + author-view
    /// + thread-view items).
    let mentionProfiles: [String: MentionProfile]
    let onRefresh: () -> Void
    let onLike: (String) -> Void
    /// NIP-57 — (eventID, authorPubkey, lnurl) → dispatch the zap. The row
    /// only surfaces the button when `authorLnurl != nil`, so this closure
    /// is always called with a non-empty `lnurl`. Threaded through alongside
    /// `onLike` to avoid coupling the row to `KernelModel` directly.
    let onZap: (String, String, String) -> Void
    let onLoadMore: (TimelineWindowCursor) -> Void

    nonisolated static func == (lhs: TimelineListView, rhs: TimelineListView) -> Bool {
        lhs.blocks == rhs.blocks
            && lhs.cards == rhs.cards
            && lhs.nextCursor == rhs.nextCursor
            && lhs.items == rhs.items
            && lhs.mentionProfiles == rhs.mentionProfiles
    }

    var body: some View {
        let cardLookup = Dictionary(uniqueKeysWithValues: cards.map { ($0.id, $0) })
        let itemLookup = Dictionary(uniqueKeysWithValues: items.map { ($0.id, $0) })

        return List {
            ForEach(Array(blocks.enumerated()), id: \.element.stableID) { index, block in
                ModularBlockView(
                    block: block,
                    cards: cardLookup,
                    items: itemLookup,
                    mentionProfiles: mentionProfiles,
                    onLike: onLike,
                    onZap: onZap
                )
                    .listRowInsets(EdgeInsets())
                    .listRowSeparator(.hidden)
                    .listRowBackground(ChirpColor.bg)
                    .onAppear {
                        if index == blocks.count - 1, let cursor = nextCursor {
                            onLoadMore(cursor)
                        }
                    }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .contentMargins(.bottom, 20, for: .scrollContent)
        .accessibilityIdentifier("timeline-list")
        .refreshable {
            onRefresh()
        }
    }
}
