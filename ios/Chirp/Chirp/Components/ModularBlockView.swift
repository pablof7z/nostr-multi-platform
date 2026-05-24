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
    /// NIP-57 — (eventID, authorPubkey, lnurl) → dispatch the zap. `nil`
    /// when the embedding host does not wire zap (kept optional so views
    /// other than the home feed don't need to thread a no-op). The row
    /// hides the zap button when the author has no kind:0 lnurl.
    var onZap: ((String, String, String) -> Void)? = nil

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
                    onLike: onLike,
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
                onLike: onLike,
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
        // V-27 thin-shell: the secondary monospaced caption is the abbreviated
        // hex pubkey shipped by Rust on the card. Falls back to "" when there
        // is no card (no abbreviated form exists for an item-only row; the
        // dual-identity row collapses to just the primary display name).
        let display = card?.authorPubkeyShort ?? ""
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
        // V-27 thin-shell: initials and colour come from Rust (`TimelineItem`
        // for the kernel's visible-items row, `ChirpEventCard` for the
        // synthetic-from-card row). The `"?"` / `"888888"` final fallbacks
        // only fire when both lookups miss — i.e., a row that wasn't shipped
        // by either projection, which the renderer should not surface anyway.
        return ChirpAvatar(
            url: item?.authorPictureUrl ?? "identicon:\(pubkey.prefix(8))",
            initials: item?.authorAvatarInitials ?? card?.authorAvatarInitials ?? "?",
            colorHex: item?.authorAvatarColor ?? card?.authorAvatarColor ?? "888888",
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

            // V-27 thin-shell: both `TimelineItem` and `ChirpEventCard` now
            // carry `createdAtDisplay` computed in Rust.
            if let ts = item?.createdAtDisplay ?? card?.createdAtDisplay {
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

    private func displayName(item: TimelineItem?, card: ChirpEventCard?) -> String {
        if let item, !item.authorDisplay.isEmpty { return item.authorDisplay }
        return card?.authorDisplayName ?? "Unknown"
    }

    private func syntheticItem(card: ChirpEventCard, item: TimelineItem?) -> TimelineItem {
        // `isRepost` / `navTargetId` / `repostInnerContent` are computed by
        // Rust on real timeline rows; synthetic embedded-card items are not
        // surfaced through the repost rendering path (they back the inline
        // `EmbeddedNostrEventCard`, not a row), so we feed neutral defaults
        // that match the Rust fallback for kind:1 — no inner-event parsing
        // here either.
        //
        // V6 Stage 3 partial (F-05): `TimelineItem` is now generated. The
        // three formerly-optional fallbacks (`authorPictureUrl`, etc.) are
        // non-optional in the generated shape, so the synthetic builder
        // provides explicit fallbacks here instead of relying on the
        // hand-written struct's `decodeIfPresent ??` defaults. Mirrors the
        // Rust `identicon:<prefix>` placeholder contract (D1).
        // V-27 thin-shell: all formerly-Swift display strings now come from
        // Rust on the card (`createdAtDisplay`, `authorAvatarColor`,
        // `authorAvatarInitials`, `authorDisplayName`). The four helpers
        // (`defaultInitials`, `defaultColor`, `displayPubkey`, `relativeTime`)
        // are deleted.
        TimelineItem(
            authorAvatarColor: item?.authorAvatarColor ?? card.authorAvatarColor,
            authorAvatarInitials: item?.authorAvatarInitials ?? card.authorAvatarInitials,
            // `authorAvatarSource` was never decoded by the hand-written
            // struct; for a synthetic card we mirror the Rust placeholder
            // discriminator (`"kind0"` when the source item already carried
            // a kind:0-backed avatar, `"placeholder"` otherwise).
            authorAvatarSource: item?.authorAvatarSource ?? "placeholder",
            authorDisplay: item?.authorDisplay ?? card.authorDisplayName,
            // Inherit lnurl from the cached TimelineItem when present so a
            // synthetic-from-card row still exposes the zap affordance.
            // `nil` for cards without a backing item is correct — the row
            // hides the zap button (no lnurl known yet).
            authorLnurl: item?.authorLnurl,
            // V-32 thin-shell: Rust ships `author_picture_url` on the card —
            // it is either the kind:0 `picture` URL or the cross-surface
            // `identicon:<first 16>` placeholder from
            // `nmp_core::substrate::picture_placeholder`. The old Swift
            // fallback constructed `identicon:<first 8>`; the move to the
            // 16-char `picture_placeholder` prefix is a deliberate alignment
            // (same precedent as V-27's avatar-colour algorithm fix).
            authorPictureUrl: item?.authorPictureUrl ?? card.authorPictureUrl,
            authorPubkey: card.authorPubkey,
            // V-28 thin-shell: bind the Rust-pre-formatted abbreviation
            // verbatim from the backing item when present, else from the
            // card's V-28 field. Never slice the raw pubkey in Swift.
            authorPubkeyShort: item?.authorPubkeyShort ?? card.authorPubkeyShort,
            content: card.content,
            // V-32 thin-shell: Rust ships the first 180 chars of content as
            // `content_preview` on the card so this synthetic builder no
            // longer slices the raw `content` string in Swift.
            contentPreview: card.contentPreview,
            createdAtDisplay: card.createdAtDisplay,
            id: card.id,
            isRepost: false,
            kind: card.kind,
            navTargetId: card.id,
            relayCount: 0,
            repostInnerContent: "",
            // V-28 thin-shell: same precedence — backing item's `shortId`
            // wins, falling back to the card's Rust-pre-formatted field.
            shortId: item?.shortId ?? card.shortId
        )
    }

    private func truncate(_ s: String, _ n: Int) -> String {
        s.count <= n ? s : String(s.prefix(n)) + "…"
    }
}
