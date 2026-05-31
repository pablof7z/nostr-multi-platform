import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// HomeFeedView — Home timeline root for Chirp.
//
// V-80 rung 7 — the home feed is thread-ROOTS-only. It renders
// `model.modularTimeline.cards` (`[ChirpRootCard]`): one row per thread root.
// Each root delegates to the existing `ModularBlockView` standalone path (so
// the tweet surface — font, padding, action buttons — is unchanged) and, when
// follows replied in the thread, shows a "↳ <name> replied in thread"
// attribution line above the row. A followed user's reply to a non-followed
// author's note surfaces THAT note here; replies never get their own row.
//
// chirp-tui shows the most-recent 1 replier; iOS likewise shows the most
// recent here (the projection carries all repliers raw — Q1 display decision).
//
// Empty state and pull-to-refresh stay unchanged. The per-row card lookup is a
// single-entry dictionary built per row — cards are small (≤ visible_limit;
// ≤80 by default), so the renderer doesn't need to memoize.
// ─────────────────────────────────────────────────────────────────────────

/// V-106 — the zap target captured when the user taps the zap button, held in
/// `HomeFeedView` state to drive the `ZapAmountSheet`. Carries only the raw
/// identifiers the kernel needs (eventID, authorPubkey, lnurl); the amount is
/// chosen in the sheet. `Identifiable` keyed on the target event so SwiftUI's
/// `sheet(item:)` re-presents correctly when a different note is zapped.
struct PendingZap: Identifiable {
    let eventID: String
    let authorPubkey: String
    let lnurl: String
    var id: String { eventID }
}

struct HomeFeedView: View {
    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    /// Controls the top-level "new note" compose sheet (toolbar button).
    @State private var showCompose = false
    /// Controls the publish outbox sheet.
    @State private var showOutbox = false
    /// V-106 — the zap target awaiting an amount selection. Non-nil drives the
    /// `ZapAmountSheet` presentation; the row's `onZap` closure populates it
    /// (the kernel still owns relay selection + LNURL — the sheet only picks
    /// the msats amount + optional comment).
    @State private var pendingZap: PendingZap?

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
        // V-106 — amount picker. `item:` binds to the pending zap target so the
        // sheet's `onConfirm` has the (eventID, pubkey, lnurl) captured at tap
        // time; it supplies only the chosen msats amount + optional comment.
        .sheet(item: $pendingZap) { target in
            ZapAmountSheet { amountMsats, comment in
                model.zap(
                    targetEventID: target.eventID,
                    authorPubkey: target.authorPubkey,
                    lnurl: target.lnurl,
                    amountMsats: amountMsats,
                    comment: comment
                )
            }
        }
    }

    // V-80 — empty when the OP feed has produced no root cards. The legacy
    // flat-list cold-boot fallback is gone: the engine surfaces every
    // root-shaped event as a card directly (no `timeline_authors` gate on
    // roots), so the empty state shows only until the first root lands.
    private var isEmpty: Bool {
        model.modularTimeline.cards.isEmpty
    }

    private var currentAccount: AccountSummary? {
        guard let activeID = model.activeAccount else { return nil }
        return model.accounts.first { $0.id == activeID }
    }

    // ── Timeline list ──────────────────────────────────────────────────────

    private var timeline: some View {
        TimelineListView(
            roots: model.modularTimeline.cards,
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
            onRepost: { eventID, pubkey in model.repost(eventID: eventID, authorPubkey: pubkey) },
            // NIP-57 (V-106) — tapping zap opens the amount picker rather than
            // firing a fixed 21-sat zap. `lnurl` is the pre-extracted
            // `authorLnurl` from the timeline item (Rust decides zapability;
            // the row only surfaces this closure when the field is non-nil —
            // see `NoteActionsRow`). The actual dispatch happens in the sheet's
            // `onConfirm` once the user picks an amount.
            onZap: { eventID, pubkey, lnurl in
                pendingZap = PendingZap(eventID: eventID, authorPubkey: pubkey, lnurl: lnurl)
            },
            onLoadMore: { cursor in
                model.loadOlderTimeline(after: cursor)
            }
        )
        .equatable()
    }

    // ── Empty / loading state ─────────────────────────────────────────────

    private var emptyState: some View {
        ScrollView {
            ChirpPlaceholder(
                systemImage: "bird",
                title: "Your timeline",
                subtitle: "Nothing here yet."
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
                        pubkey: account.id,
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
    /// V-80 — one entry per thread root (`RootFeedSnapshot.cards`). Each root
    /// renders as a single standalone row plus an optional attribution line.
    let roots: [ChirpRootCard]
    let nextCursor: TimelineWindowCursor?
    let items: [TimelineItem]
    /// V-31 — kernel-owned profile map (replaces the Swift
    /// `Dictionary(items.map …)` derivation this view used to build). Bound
    /// from `model.mentionProfiles`, which reads the pre-merged
    /// `resolved_profiles` snapshot projection (PR #812 — claimed +
    /// author_view + mention, merged once in Rust).
    let mentionProfiles: [String: MentionProfile]
    let onRefresh: () -> Void
    let onLike: (String) -> Void
    let onRepost: (String, String) -> Void
    /// NIP-57 — (eventID, authorPubkey, lnurl) → dispatch the zap. The row
    /// only surfaces the button when `authorLnurl != nil`, so this closure
    /// is always called with a non-empty `lnurl`. Threaded through alongside
    /// `onLike` to avoid coupling the row to `KernelModel` directly.
    let onZap: (String, String, String) -> Void
    let onLoadMore: (TimelineWindowCursor) -> Void

    nonisolated static func == (lhs: TimelineListView, rhs: TimelineListView) -> Bool {
        lhs.roots == rhs.roots
            && lhs.nextCursor == rhs.nextCursor
            && lhs.items.count == rhs.items.count
            && zip(lhs.items, rhs.items).allSatisfy { $0.rendersIdentically(to: $1) }
            && lhs.mentionProfiles == rhs.mentionProfiles
    }

    var body: some View {
        let itemLookup = Dictionary(uniqueKeysWithValues: items.map { ($0.id, $0) })

        return List {
            ForEach(Array(roots.enumerated()), id: \.element.id) { index, root in
                VStack(alignment: .leading, spacing: 0) {
                    // Q1 — show the most-recent replier (the engine orders the
                    // attribution Vec oldest-first, so `.last` is newest).
                    if let attribution = root.attribution.last {
                        attributionLine(attribution)
                    }
                    ModularBlockView(
                        // Reuse the standalone render path: the root card id is
                        // the row id (for reposts the engine forced it to the
                        // superseded target id). A single-entry card lookup
                        // feeds the existing renderer.
                        block: .standalone(eventID: root.card.id, root: nil),
                        cards: [root.card.id: root.card],
                        items: itemLookup,
                        mentionProfiles: mentionProfiles,
                        onLike: onLike,
                        onRepost: onRepost,
                        onZap: onZap
                    )
                }
                    .listRowInsets(EdgeInsets())
                    .listRowSeparator(.hidden)
                    .listRowBackground(ChirpColor.bg)
                    .onAppear {
                        if index == roots.count - 1, let cursor = nextCursor {
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

    /// "↳ <name> replied in thread" — surfaces the follow whose reply caused
    /// this root to appear (or who replied to it). `authorDisplayName` falls
    /// back to the abbreviated raw pubkey when no kind:0 has arrived yet
    /// (ADR-0032 display separation).
    private func attributionLine(_ attribution: ChirpReplyAttribution) -> some View {
        let name = attribution.authorDisplayName?.isEmpty == false
            ? attribution.authorDisplayName!
            : attribution.authorPubkey.shortHex
        return HStack(spacing: 4) {
            Image(systemName: "arrow.turn.down.right")
                .font(.caption2)
                .foregroundStyle(ChirpColor.link)
            Text("\(name) replied in thread")
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .padding(.horizontal, 16)
        .padding(.top, 8)
        .accessibilityIdentifier("thread-attribution-\(attribution.replyEventId.prefix(8))")
    }
}
