import SwiftUI

/// Surfaces the agent's Nostr conversation activity on the main screen:
///   • a floating "Talking to X" capsule while a turn is fresh, and
///   • a toolbar button that opens `NostrConversationsView`, badged with
///     the unread-since-last-viewed count.
///
/// Composition: the overlay + sheet + last-viewed bookkeeping live in
/// `NostrAgentSurface` (applied as a view modifier on `RootView.body`).
/// The toolbar button is a separate `ToolbarContent` so it can slot into
/// each tab's `NavigationStack` toolbar — toolbars don't propagate from
/// outside a `NavigationStack`. They communicate via a notification so
/// neither needs a binding into the other.

extension Notification.Name {
    /// Posted by `NostrConversationsToolbarItem` when the user taps the
    /// conversations button. `NostrAgentSurface` listens and presents the
    /// `NostrConversationsView` sheet.
    static let openNostrConversationsRequested = Notification.Name("openNostrConversationsRequested")
}

// MARK: - Activity indicator

/// Floating capsule shown at the top of the screen while a Nostr
/// conversation turn is fresh (incoming or outgoing). Disappears 10s
/// after the latest turn; each new turn resets the timer via
/// `AppStateStore.noteNostrActivity(counterpartyPubkey:)`.
struct NostrActivityIndicator: View {
    let counterpartyPubkey: String
    let profile: NostrProfileMetadata?

    var body: some View {
        HStack(spacing: AppTheme.Spacing.sm + 2) {
            NostrProfileAvatar(profile: profile)
                .frame(width: 24, height: 24)
            Text("Talking to \(displayName)")
                .font(.subheadline.weight(.medium))
                .lineLimit(1)
                .truncationMode(.middle)
        }
        .padding(.horizontal, AppTheme.Spacing.md - 4)
        .padding(.vertical, AppTheme.Spacing.sm)
        .glassEffect(.regular, in: .capsule)
    }

    private var displayName: String {
        if let label = profile?.bestLabel, !label.isEmpty { return label }
        return NostrNpub.shortNpub(fromHex: counterpartyPubkey)
    }
}

// MARK: - Toolbar item

/// Conversations button shown next to the agent + settings toolbar
/// buttons. Visible while any conversation has been touched in the last
/// 15 minutes (or has unread turns since the user last opened the
/// sheet). Badged with the unread count.
struct NostrConversationsToolbarItem: ToolbarContent {
    @Environment(AppStateStore.self) private var store
    @AppStorage("nostrConversationsLastViewedAt") private var lastViewedAt: Double = 0

    var body: some ToolbarContent {
        ToolbarItem(placement: .topBarTrailing) {
            if shouldShow {
                Button {
                    Haptics.selection()
                    NotificationCenter.default.post(name: .openNostrConversationsRequested, object: nil)
                } label: {
                    Image(systemName: "bubble.left.and.bubble.right")
                        .overlay(alignment: .topTrailing) {
                            if unreadCount > 0 {
                                Text("\(unreadCount)")
                                    .font(.system(size: 10, weight: .bold))
                                    .foregroundStyle(.white)
                                    .frame(width: 16, height: 16)
                                    .background(Color.red, in: Circle())
                                    .offset(x: 6, y: -6)
                            }
                        }
                }
                .accessibilityLabel(unreadCount > 0 ? "Conversations — \(unreadCount) new" : "Conversations")
            }
        }
    }

    private var shouldShow: Bool {
        let fifteenMinutesAgo = Date().addingTimeInterval(-15 * 60)
        let recent = store.state.nostrConversations.contains { $0.lastTouched > fifteenMinutesAgo }
        return unreadCount > 0 || recent
    }

    private var unreadCount: Int {
        let lastViewed = Date(timeIntervalSince1970: lastViewedAt)
        return store.state.nostrConversations.filter { $0.lastTouched > lastViewed }.count
    }
}

// MARK: - Surface modifier

/// Wraps `RootView` with the activity-indicator overlay and the
/// conversations sheet. Listens for `openNostrConversationsRequested`
/// to present the sheet, and stamps the last-viewed timestamp on
/// dismiss so the toolbar badge clears.
struct NostrAgentSurface: ViewModifier {
    @Environment(AppStateStore.self) private var store
    @AppStorage("nostrConversationsLastViewedAt") private var lastViewedAt: Double = 0
    @State private var showConversations = false

    func body(content: Content) -> some View {
        content
            .overlay(alignment: .top) { indicatorOverlay }
            .animation(AppTheme.Animation.springFast, value: store.activeNostrCounterparty)
            .onReceive(NotificationCenter.default.publisher(for: .openNostrConversationsRequested)) { _ in
                lastViewedAt = Date().timeIntervalSince1970
                showConversations = true
            }
            .sheet(isPresented: $showConversations) {
                NavigationStack {
                    NostrConversationsView()
                }
            }
    }

    @ViewBuilder
    private var indicatorOverlay: some View {
        if let counterparty = store.activeNostrCounterparty {
            NostrActivityIndicator(
                counterpartyPubkey: counterparty,
                profile: store.state.nostrProfileCache[counterparty]
            )
            .padding(.top, AppTheme.Spacing.sm)
            .transition(.move(edge: .top).combined(with: .opacity))
        }
    }
}

extension View {
    /// Attaches the floating "Talking to X" indicator and the Nostr
    /// conversations sheet to the main screen. Apply once at the
    /// `RootView.body` level — the matching toolbar button
    /// (`NostrConversationsToolbarItem`) slots into each tab's
    /// `NavigationStack` separately.
    func nostrAgentSurface() -> some View {
        modifier(NostrAgentSurface())
    }
}
