import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// T146 — Renders one `TimelineBlock` from the Chirp modular timeline.
//
// `Standalone` falls through to the existing `NoteRowView` so the tweet
// surface (font, padding, action buttons, divider) is byte-identical to
// the pre-modular look.
//
// `Module` renders the chained events vertically:
//   ●  @alice
//   │   Original tweet text...
//   │
//   ●  @bob
//       Reply text...
//   [Show this thread]   (if hasGap or root mismatches the chain top)
//
// The vertical line is a 1.5pt rounded rect in `ChirpColor.hairline`-ish
// territory (system tertiary label), positioned in the avatar column so it
// connects the bottom of avatar N to the top of avatar N+1. Self-thread vs
// cross-author render the same shape; the renderer suppresses the
// "Replying to @x" header on a self-thread (would be tautological).
// ─────────────────────────────────────────────────────────────────────────

struct ModularBlockView: View {
    let block: TimelineBlock
    let cards: [String: ChirpEventCard]
    /// Lookup into the kernel's existing TimelineItem snapshot for author
    /// display / avatar metadata. A missing entry falls back to the card's
    /// raw pubkey (D1 placeholders apply: identicon + truncated npub).
    let items: [String: TimelineItem]

    @EnvironmentObject private var router: ChirpRouter

    var body: some View {
        switch block {
        case .standalone(let id):
            standaloneRow(id: id)
        case .module(let events, let hasGap, let root):
            moduleStack(events: events, hasGap: hasGap, root: root)
        }
    }

    // ── Standalone — delegate to the existing NoteRowView ────────────────

    @ViewBuilder
    private func standaloneRow(id: String) -> some View {
        if let item = items[id] {
            NoteRowView(item: item)
        } else if let card = cards[id] {
            // Card without a TimelineItem: build a synthetic item so the
            // standalone path stays consistent. This happens when an
            // ancestor of a reply lands but isn't in the kernel's visible
            // window (timeline_authors filter, visible_limit, etc.).
            NoteRowView(item: syntheticItem(card: card, item: nil))
        } else {
            // Neither cached locally nor available as a kernel item — show
            // a minimal placeholder so the row count stays consistent.
            EmptyView()
        }
    }

    // ── Module stack with vertical connecting line ───────────────────────

    private func moduleStack(events: [String], hasGap: Bool, root: ThreadPointer?) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(events.enumerated()), id: \.element) { (index, id) in
                let isLast = (index == events.count - 1)
                moduleRow(id: id, isLast: isLast)
            }

            if shouldShowGapPill(hasGap: hasGap, root: root, events: events) {
                showThisThreadPill(rootID: rootEventID(root: root) ?? events.first ?? "")
                    .padding(.leading, ChirpSpace.l + 44 + ChirpSpace.m)
                    .padding(.bottom, ChirpSpace.m)
            }

            Divider()
                .background(ChirpColor.hairline)
        }
        .padding(.vertical, ChirpSpace.m)
        .padding(.horizontal, ChirpSpace.l)
        .listRowInsets(EdgeInsets())
        .listRowSeparator(.hidden)
        .listRowBackground(Color.clear)
    }

    private func moduleRow(id: String, isLast: Bool) -> some View {
        let item = items[id]
        let card = cards[id]
        let display = displayPubkey(item: item, card: card)
        let content = item?.content ?? card?.content ?? ""

        // Tap = navigate to the thread for this event id (legacy
        // `ThreadScreen` consumes `ThreadViewPayload` — the M2 migration of
        // the thread surface itself is out of scope for this PR). Same
        // affordance the existing `NoteRowView` provides on standalone
        // rows; without it module rows would silently swallow taps.
        return Button {
            router.push(.thread(eventID: id))
        } label: {
            HStack(alignment: .top, spacing: ChirpSpace.m) {
                avatarColumn(item: item, card: card, isLast: isLast)
                VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                    authorHeader(display: display, item: item, card: card)
                    if !content.isEmpty {
                        NoteContentView(content: truncate(content, 1_200), font: ChirpFont.body)
                            .foregroundStyle(ChirpColor.textPrimary)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .padding(.bottom, isLast ? 0 : ChirpSpace.m)
    }

    // The avatar column carries both the avatar and (for all but the last
    // row) the connecting vertical line. Implementing it as a `ZStack` keeps
    // the line in the avatar's horizontal lane without forcing each row to
    // calculate its own neighbour's geometry — the line spans the full
    // height of the row below the avatar.
    private func avatarColumn(item: TimelineItem?, card: ChirpEventCard?, isLast: Bool) -> some View {
        let pubkey = item?.authorPubkey ?? card?.authorPubkey ?? ""
        return ZStack(alignment: .top) {
            // Vertical line — drawn first so the avatar overlays it. Skip
            // the line on the last row of the module (no successor).
            if !isLast {
                GeometryReader { geo in
                    RoundedRectangle(cornerRadius: 1)
                        .fill(Color(uiColor: .tertiaryLabel))
                        .frame(width: 1.5,
                               height: geo.size.height - 44 + ChirpSpace.m + ChirpSpace.m)
                        .position(x: 22, // half of 44pt avatar
                                  y: 44 + (geo.size.height - 44) / 2)
                }
                .frame(width: 44)
            }
            ChirpAvatar(
                url: item?.authorPictureUrl ?? "identicon:\(pubkey.prefix(8))",
                initials: item?.authorAvatarInitials ?? defaultInitials(pubkey: pubkey),
                colorHex: item?.authorAvatarColor ?? defaultColor(pubkey: pubkey),
                size: 44
            )
        }
        .frame(width: 44, alignment: .top)
    }

    private func authorHeader(display: String, item: TimelineItem?, card: ChirpEventCard?) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: ChirpSpace.xs) {
            Text(displayName(item: item, card: card))
                .font(ChirpFont.headline)
                .foregroundStyle(ChirpColor.textPrimary)
                .lineLimit(1)

            Text(display)
                .font(ChirpFont.mono)
                .foregroundStyle(ChirpColor.textTertiary)
                .lineLimit(1)

            Spacer(minLength: 0)

            if let ts = item?.createdAtDisplay ?? card.map(relativeTime(card:)) {
                Text(ts)
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textSecondary)
            }
        }
    }

    private func showThisThreadPill(rootID: String) -> some View {
        // Tap drops the user into ThreadScreen anchored at the chain's
        // resolved root (or the chain top when `root` is nil — see
        // `rootEventID(root:)` for the precedence). ThreadScreen still
        // consumes the legacy `ThreadViewPayload`; that migration is
        // explicitly out of scope for this PR (M2 follow-up).
        Button {
            router.push(.thread(eventID: rootID))
        } label: {
            Text("Show this thread")
                .font(ChirpFont.caption)
                .foregroundStyle(ChirpColor.accent)
                .padding(.horizontal, ChirpSpace.m)
                .padding(.vertical, 4)
                .background(ChirpColor.accent.opacity(0.1), in: Capsule())
        }
        .buttonStyle(.borderless)
        .accessibilityIdentifier("show-this-thread-\(rootID.prefix(8))")
    }

    // ── Helpers ───────────────────────────────────────────────────────────

    private func shouldShowGapPill(hasGap: Bool, root: ThreadPointer?, events: [String]) -> Bool {
        if hasGap { return true }
        if let rootID = rootEventID(root: root), let topID = events.first, rootID != topID {
            return true
        }
        return false
    }

    private func rootEventID(root: ThreadPointer?) -> String? {
        root?.eventID
    }

    private func displayPubkey(item: TimelineItem?, card: ChirpEventCard?) -> String {
        let hex = item?.authorPubkey ?? card?.authorPubkey ?? ""
        guard hex.count >= 12 else { return hex }
        return "\(hex.prefix(6))…\(hex.suffix(4))"
    }

    private func displayName(item: TimelineItem?, card: ChirpEventCard?) -> String {
        if let item, !item.authorDisplay.isEmpty { return item.authorDisplay }
        // No profile yet — use the truncated pubkey as a fallback (matches
        // the `short_pubkey_display` behaviour of the Rust side).
        let hex = card?.authorPubkey ?? item?.authorPubkey ?? ""
        return hex.isEmpty ? "Unknown" : "\(hex.prefix(8))…"
    }

    private func relativeTime(card: ChirpEventCard) -> String {
        let now = Date().timeIntervalSince1970
        let then = TimeInterval(card.createdAt)
        let delta = max(0, now - then)
        if delta < 60 { return "\(Int(delta))s" }
        if delta < 3600 { return "\(Int(delta / 60))m" }
        if delta < 86_400 { return "\(Int(delta / 3600))h" }
        return "\(Int(delta / 86_400))d"
    }

    private func syntheticItem(card: ChirpEventCard, item: TimelineItem?) -> TimelineItem {
        TimelineItem(
            id: card.id,
            authorPubkey: card.authorPubkey,
            authorDisplay: item?.authorDisplay ?? "\(card.authorPubkey.prefix(8))…",
            authorPictureUrl: item?.authorPictureUrl,
            authorAvatarInitials: item?.authorAvatarInitials ?? defaultInitials(pubkey: card.authorPubkey),
            authorAvatarColor: item?.authorAvatarColor ?? defaultColor(pubkey: card.authorPubkey),
            content: card.content,
            contentPreview: String(card.content.prefix(180)),
            createdAtDisplay: relativeTime(card: card),
            relayCount: 0
        )
    }

    private func defaultInitials(pubkey: String) -> String {
        String(pubkey.prefix(2))
    }

    private func defaultColor(pubkey: String) -> String {
        // Deterministic-ish hex from the first two chars; mirrors the
        // Rust `avatar_color` function's purpose without duplicating its
        // exact algorithm (D6 — display field is best-effort).
        "#" + String(pubkey.prefix(6))
    }

    private func truncate(_ s: String, _ n: Int) -> String {
        s.count <= n ? s : String(s.prefix(n)) + "…"
    }
}
