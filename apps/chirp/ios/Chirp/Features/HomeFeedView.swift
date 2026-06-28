import SwiftUI
import UIKit

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
    /// Controls the NIP-50 search sheet (magnifier toolbar button).
    @State private var showSearch = false
    /// V-106 — the zap target awaiting an amount selection. Non-nil drives the
    /// `ZapAmountSheet` presentation; the row's `onZap` closure populates it
    /// (the kernel still owns relay selection + LNURL — the sheet only picks
    /// the msats amount + optional comment).
    @State private var pendingZap: PendingZap?
    /// Correlation id of the most recent zap dispatch, set when the
    /// `ZapAmountSheet` fires. Observed via `model.actionLifecycle` to
    /// surface success feedback (haptic + toast) once the NWC kind:23195
    /// response closes the action with `Accepted`.
    @State private var pendingZapCid: String?

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
        .sheet(isPresented: $showSearch) {
            SearchSheet()
        }
        // V-106 — amount picker. `item:` binds to the pending zap target so the
        // sheet's `onConfirm` has the (eventID, pubkey, lnurl) captured at tap
        // time; it supplies only the chosen msats amount + optional comment.
        .sheet(item: $pendingZap) { target in
            ZapAmountSheet { amountMsats, comment in
                let result = model.zap(
                    targetEventID: target.eventID,
                    authorPubkey: target.authorPubkey,
                    lnurl: target.lnurl,
                    amountMsats: amountMsats,
                    comment: comment
                )
                pendingZapCid = result.correlationId
            }
        }
        // Observe the action lifecycle to give feedback when the NWC payment
        // settles. Error paths (no wallet, LNURL failure, bunker account)
        // already surface via `lastErrorToast` from Rust's `ShowToast`
        // command — we only need to handle the `Accepted` leg here.
        .onChange(of: model.actionLifecycle) {
            guard let cid = pendingZapCid,
                  let terminal = model.recentTerminal(correlationId: cid) else { return }
            pendingZapCid = nil
            if case .accepted = terminal.stage {
                UINotificationFeedbackGenerator().notificationOccurred(.success)
                model.showSuccessToast("⚡ Zapped!")
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
            onRefresh: { model.openTimeline() },
            onLike: { model.react(targetEventID: $0, reaction: "❤") },
            onRepost: { eventID, pubkey in model.repost(eventID: eventID, authorPubkey: pubkey) },
            // NIP-57 (V-106) — tapping zap opens the amount picker rather than
            // firing a fixed 21-sat zap. `lnurl` is the pre-extracted keyed
            // profile-sidecar value (Rust decides zapability; the row only
            // surfaces this closure when the field is non-nil — see
            // `NoteActionsRow`). The actual dispatch happens in the sheet's
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
                    NostrAvatar(
                        pubkey: account.id,
                        url: account.pictureUrl,
                        size: 32
                    )
                    .equatable()
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
                showSearch = true
            } label: {
                Image(systemName: "magnifyingglass")
            }
            .accessibilityLabel("Search")
            .accessibilityIdentifier("search-button")
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
    let onRefresh: () -> Void
    let onLike: (String) -> Void
    let onRepost: (String, String) -> Void
    /// NIP-57 — (eventID, authorPubkey, lnurl) → dispatch the zap. The row
    /// only surfaces the button when the keyed profile sidecar has `lnurl`, so
    /// this closure is always called with a non-empty `lnurl`. Threaded through
    /// alongside `onLike` to avoid coupling the row to `KernelModel` directly.
    let onZap: (String, String, String) -> Void
    let onLoadMore: (TimelineWindowCursor) -> Void

    nonisolated static func == (lhs: TimelineListView, rhs: TimelineListView) -> Bool {
        // ADR-0063 Lane E (#1671): `mentionProfiles` is gone from the equality
        // key. It was a whole-map profile dictionary, so any profile update
        // changed it and re-rendered the entire timeline list. Profiles now
        // re-render per-key inside each note's `NoteContentView` via the keyed
        // cache observer — the list itself only changes on roots/cursor.
        lhs.roots == rhs.roots
            && lhs.nextCursor == rhs.nextCursor
    }

    var body: some View {
        List {
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
    /// this root to appear (or who replied to it).
    private func attributionLine(_ attribution: ChirpReplyAttribution) -> some View {
        ReplyAttributionLine(attribution: attribution)
    }
}

/// One reply-attribution line ("↳ <name> replied in thread").
///
/// F-CR-00 claim-only invariant: the attributed replier is an author-displaying
/// surface, so this view self-claims that pubkey's kind:0 and releases on
/// disappear — mirroring `NostrAvatar`'s lifecycle exactly. The kernel owns all
/// resolution (10002 → author relays → kind:0 REQ) off the claim alone; this
/// view only triggers it and reads the resolved name back reactively.
///
/// The displayed name prefers the live host projection, then the snapshot-baked
/// `authorDisplayName`, then the Rust-formatted `shortHex` — never blank while
/// the claim is in flight (ADR-0032 display separation).
private struct ReplyAttributionLine: View {
    @Environment(\.nostrProfileHost) private var profileHost

    let attribution: ChirpReplyAttribution

    @State private var consumerID = "home-feed.reply-attribution.\(UUID().uuidString)"
    @State private var claimedPubkey: String?
    /// ADR-0063 Lane E (#1671): per-key observer so only this attribution line
    /// re-renders when its author's `refs.profile` row commits.
    @StateObject private var rowObserver = KeyedRefRowObserver()

    private var name: String {
        if let live = profileHost?.profile(forPubkey: attribution.authorPubkey)?.displayName,
            !live.isEmpty {
            return live
        }
        if let baked = attribution.authorDisplayName, !baked.isEmpty {
            return baked
        }
        return attribution.authorPubkey.shortHex
    }

    var body: some View {
        HStack(spacing: 4) {
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
        .task(id: attribution.authorPubkey) {
            await MainActor.run {
                if let claimedPubkey, claimedPubkey != attribution.authorPubkey {
                    profileHost?.releaseProfile(pubkey: claimedPubkey, consumerID: consumerID)
                }
                claimedPubkey = attribution.authorPubkey
                // ADR-0063 Lane E (#1671): attribution line is a feed/list
                // context → `resolve_ref(Profile, …, .profileRef, .cacheOk)`.
                if let host = profileHost {
                    rowObserver.observe(host.profileRowChanged, pubkey: attribution.authorPubkey)
                }
                profileHost?.resolveProfile(
                    pubkey: attribution.authorPubkey,
                    consumerID: consumerID,
                    shape: .profileRef,
                    liveness: .cacheOk)
            }
        }
        .onDisappear {
            if let claimedPubkey {
                profileHost?.releaseProfile(pubkey: claimedPubkey, consumerID: consumerID)
                self.claimedPubkey = nil
            }
        }
    }
}
