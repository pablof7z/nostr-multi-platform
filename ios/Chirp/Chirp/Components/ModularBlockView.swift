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
//     "Replying to @x" header that non-module reply rows show is suppressed
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
    /// NIP-18 — (eventID, authorPubkey) → dispatch kind:6 repost.
    var onRepost: ((String, String) -> Void)? = nil
    /// NIP-57 — (eventID, authorPubkey, lnurl) → dispatch the zap. `nil`
    /// when the embedding host does not wire zap (kept optional so views
    /// other than the home feed don't need to thread a no-op). The row
    /// hides the zap button when the author has no kind:0 lnurl.
    var onZap: ((String, String, String) -> Void)? = nil

    @EnvironmentObject private var router: ChirpRouter
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        switch block {
        case .standalone(let id, _):
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
                    relationCounts: cards[id]?.relationCounts,
                    onLike: onLike,
                    onRepost: onRepost,
                    onZap: onZap
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
                relationCounts: card.relationCounts,
                onLike: onLike,
                onRepost: onRepost,
                onZap: onZap
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
        // ADR-0032: presentation layer derives the secondary monospaced
        // pubkey label from the raw hex pubkey it already has on hand.
        let pubkey = item?.authorPubkey ?? card?.authorPubkey ?? ""
        // Resolve the display name through the same lookup the rest of the
        // view uses (resolved_profiles → card name → shortHex). Previously
        // hardcoded `pubkey.shortHex`, which ignored every known profile.
        let display = displayName(item: item, card: card)
        let content = displayContent(item: item, card: card)
        let context = NoteRenderContext(
            mentionProfiles: mentionProfiles,
            eventCards: cards,
            timelineItems: items
        )

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
                            contentTree: displayContentTree(item: item, card: card, context: context),
                            renderContext: context,
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
        .onAppear { model.claimVisibleNoteRelations(eventID: id) }
        .onDisappear { model.releaseVisibleNoteRelations(eventID: id) }
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
        // ADR-0032: initials and avatar tint are derived locally from the
        // raw pubkey hex via `PubkeyFormatting.swift`. Picture URL falls
        // back to the identicon URI when no kind:0 has arrived.
        let pictureUrl = item?.authorPictureUrl
            ?? card?.authorPictureUrl
            ?? "identicon:\(pubkey.prefix(16))"
        return ChirpAvatar(
            pubkey: pubkey,
            url: pictureUrl,
            initials: pubkey.displayInitials,
            colorHex: pubkey.pubkeyColorHex,
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

            // ADR-0032: both `TimelineItem` and `ChirpEventCard` ship the
            // raw `created_at` (Unix seconds); the presentation layer
            // formats the relative-time label.
            if let createdAt = item?.createdAt ?? card?.createdAt {
                Text(createdAt.relativeTimeFromUnixSeconds)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func showThisThreadPill(rootID: String) -> some View {
        // Tap drops the user into ThreadScreen anchored at the chain's
        // resolved root (or the chain top when `root` is nil — see
        // `rootEventID(root:)` for the precedence).
        Button {
            router.push(.thread(eventID: rootID))
        } label: {
            Text("Show this thread")
                .font(.caption)
                .foregroundStyle(ChirpColor.link)
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

    private func displayName(item: TimelineItem?, card: ChirpEventCard?) -> String {
        let pubkey = item?.authorPubkey ?? card?.authorPubkey ?? ""
        if !pubkey.isEmpty, let name = model.profile(forPubkey: pubkey)?.display {
            return name
        }
        if let name = card?.authorDisplayName, !name.isEmpty { return name }
        return pubkey.isEmpty ? "Unknown" : pubkey.shortHex
    }

    private func displayContent(item: TimelineItem?, card: ChirpEventCard?) -> String {
        if let item, item.isRepost {
            return card?.content ?? item.repostInnerContent
        }
        return item?.content ?? card?.content ?? ""
    }

    private func displayContentTree(
        item: TimelineItem?,
        card: ChirpEventCard?,
        context: NoteRenderContext
    ) -> ContentTreeWire? {
        guard let item else { return card?.contentTree }
        return context.contentTree(for: item, fallback: card?.contentTree)
    }

    private func syntheticItem(card: ChirpEventCard, item: TimelineItem?) -> TimelineItem {
        // ADR-0032: `TimelineItem` now carries raw protocol data only —
        // display formatting is the presentation layer's responsibility.
        // `isRepost` / `navTargetId` / `repostInnerContent` keep their
        // neutral kind:1 defaults; synthetic-from-card rows are not
        // surfaced through the repost rendering path.
        TimelineItem(
            // Inherit lnurl from the cached TimelineItem when present so a
            // synthetic-from-card row still exposes the zap affordance.
            // `nil` for cards without a backing item is correct — the row
            // hides the zap button (no lnurl known yet).
            authorLnurl: item?.authorLnurl,
            authorPictureUrl: item?.authorPictureUrl ?? card.authorPictureUrl,
            authorPubkey: card.authorPubkey,
            content: card.content,
            contentPreview: card.contentPreview,
            createdAt: card.createdAt,
            id: card.id,
            isRepost: false,
            kind: card.kind,
            navTargetId: card.id,
            relayCount: 0,
            repostInnerContent: ""
        )
    }

    private func truncate(_ s: String, _ n: Int) -> String {
        s.count <= n ? s : String(s.prefix(n)) + "…"
    }
}
