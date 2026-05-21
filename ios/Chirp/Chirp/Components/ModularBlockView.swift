import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// T146 — Renders one `TimelineBlock` from the Chirp modular timeline.
//
// `Standalone` falls through to the existing `NoteRowView` so the tweet
// surface (font, padding, action buttons, divider) is byte-identical to
// the pre-modular look.
//
// `Module` renders the chained events vertically, root-first newest-last:
//
//   ●  @alice
//   │   Original tweet text...
//   │
//   ●  @bob
//       Reply text...
//   [Show this thread]   (if hasGap or root mismatches the chain top)
//
// Layout invariants:
//   • Each event = one row containing a fixed-width avatar column (44pt
//     avatar + 8pt trailing) and an expanding text column.
//   • The vertical connecting line is a 1.5pt rounded rect drawn as an
//     overlay on the avatar column, anchored to the avatar's bottom edge
//     and extending downward through the inter-row spacing into the top
//     edge of the next row's avatar. Drawn for every event EXCEPT the
//     last one in the module.
//   • Self-thread vs cross-author render with the same machinery; the
//     "Replying to @x" header that legacy reply rows show is suppressed
//     here (per spec — it would be tautological inside a single block).
// ─────────────────────────────────────────────────────────────────────────

/// Module renderer constants kept together so the line geometry stays in
/// lockstep with the avatar size + row spacing.
private enum ModuleLayout {
    static let avatarSize: CGFloat = 44
    /// Vertical gap between two adjacent event rows inside a module. The
    /// line extends through this gap.
    static let interRowSpacing: CGFloat = 8
    /// Stroke width of the connecting line.
    static let lineWidth: CGFloat = 1.5
}

struct ModularBlockView: View {
    let block: TimelineBlock
    let cards: [String: ChirpEventCard]
    /// Lookup into the kernel's existing TimelineItem snapshot for author
    /// display / avatar metadata. A missing entry falls back to the card's
    /// raw pubkey (D1 placeholders apply: identicon + truncated npub).
    let items: [String: TimelineItem]
    let mentionProfiles: [String: MentionProfile]
    let onLike: (String) -> Void

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
            NoteRowView(
                    item: item,
                    contentTree: cards[id]?.contentTree,
                    mentionProfiles: mentionProfiles,
                    eventCards: cards,
                    timelineItems: items,
                    onLike: onLike
                )
        } else if let card = cards[id] {
            // Card without a TimelineItem: build a synthetic item so the
            // standalone path stays consistent. This happens when an
            // ancestor of a reply lands but isn't in the kernel's visible
            // window (timeline_authors filter, visible_limit, etc.).
            NoteRowView(
                item: syntheticItem(card: card, item: nil),
                contentTree: card.contentTree,
                mentionProfiles: mentionProfiles,
                eventCards: cards,
                timelineItems: items,
                onLike: onLike
            )
        } else {
            // Neither cached locally nor available as a kernel item — show
            // a minimal placeholder so the row count stays consistent.
            EmptyView()
        }
    }

    // ── Module stack with vertical connecting line ───────────────────────

    private func moduleStack(events: [String], hasGap: Bool, root: ThreadPointer?) -> some View {
        VStack(alignment: .leading, spacing: ModuleLayout.interRowSpacing) {
            ForEach(Array(events.enumerated()), id: \.element) { (index, id) in
                let isLast = (index == events.count - 1)
                moduleRow(id: id, isLast: isLast)
            }

            if shouldShowGapPill(hasGap: hasGap, root: root, events: events) {
                showThisThreadPill(rootID: rootEventID(root: root) ?? events.first ?? "")
                    .padding(.leading, ModuleLayout.avatarSize + 8)
                    .padding(.top, 4)
            }

            Divider()
                .padding(.leading, ModuleLayout.avatarSize + 8)
                .padding(.top, 4)
        }
        .padding(.top, 12)
        .padding(.horizontal, 16)
    }

    /// One event row inside a module. Layout: avatar column (fixed 44pt,
    /// possibly with a connecting line extending downward) + content
    /// column (expanding). The whole row is a `Button` so tap → thread,
    /// matching the affordance the existing `NoteRowView` provides on
    /// standalone blocks.
    private func moduleRow(id: String, isLast: Bool) -> some View {
        let item = items[id]
        let card = cards[id]
        let display = displayPubkey(item: item, card: card)
        let content = item?.content ?? card?.content ?? ""

        return Button {
            router.push(.thread(eventID: id))
        } label: {
            HStack(alignment: .top, spacing: 8) {
                avatarColumn(item: item, card: card, isLast: isLast)
                VStack(alignment: .leading, spacing: 4) {
                    authorHeader(display: display, item: item, card: card)
                    if !content.isEmpty {
                        NoteContentView(
                            content: truncate(content, 1_200),
                            contentTree: card?.contentTree,
                            mentionProfiles: mentionProfiles,
                            eventCards: cards,
                            timelineItems: items,
                            font: .body
                        )
                            .foregroundStyle(.primary)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    /// Avatar + the connecting line that runs from the avatar's bottom
    /// edge through the inter-row gap into the next avatar. The line is
    /// drawn as an `.overlay` on the avatar so its x-position
    /// automatically tracks the avatar centre; alignment `.bottom` +
    /// negative `.bottom` padding lets the line extend BELOW the avatar
    /// without changing the avatar's own intrinsic height. `clipped:
    /// false` is the default on `.overlay`, so the extension renders into
    /// the inter-row gap without disturbing the parent layout.
    private func avatarColumn(item: TimelineItem?, card: ChirpEventCard?, isLast: Bool) -> some View {
        let pubkey = item?.authorPubkey ?? card?.authorPubkey ?? ""
        return ChirpAvatar(
            url: item?.authorPictureUrl ?? "identicon:\(pubkey.prefix(8))",
            initials: item?.authorAvatarInitials ?? defaultInitials(pubkey: pubkey),
            colorHex: item?.authorAvatarColor ?? defaultColor(pubkey: pubkey),
            size: ModuleLayout.avatarSize
        )
        .overlay(alignment: .bottom) {
            if !isLast {
                // Connecting line runs from avatar bottom into the next
                // row's avatar top. Spans the inter-row gap (interRowSpacing)
                // and the next avatar's height to reach its centre.
                RoundedRectangle(cornerRadius: ModuleLayout.lineWidth / 2)
                    .fill(.tertiary)
                    .frame(
                        width: ModuleLayout.lineWidth,
                        height: ModuleLayout.interRowSpacing + ModuleLayout.avatarSize / 2
                    )
                    .offset(y: ModuleLayout.interRowSpacing + ModuleLayout.avatarSize / 2)
            }
        }
        .frame(width: ModuleLayout.avatarSize, alignment: .top)
    }

    private func authorHeader(display: String, item: TimelineItem?, card: ChirpEventCard?) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 4) {
            Text(displayName(item: item, card: card))
                .font(.headline)
                .foregroundStyle(.primary)
                .lineLimit(1)

            Text(display)
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
                .lineLimit(1)

            Spacer(minLength: 0)

            if let ts = item?.createdAtDisplay ?? card.map(relativeTime(card:)) {
                Text(ts)
                    .font(.caption)
                    .foregroundStyle(.secondary)
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
                .font(.caption)
                .foregroundStyle(Color.accentColor)
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
            kind: card.kind,
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
