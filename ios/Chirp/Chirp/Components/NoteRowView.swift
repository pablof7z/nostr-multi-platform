import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// NoteRowView — polished timeline cell for the Home feed.
//
// Tap targets:
//   • avatar  → router.push(.profile)
//   • whole row → router.push(.thread)
//   • action buttons (reply, repost, like, zap) → kernel commands / sheets
//
// Button nesting strategy: every inner interactive element uses
// .buttonStyle(.borderless) so its tap doesn't propagate to the row-level
// Button wrapper. The row itself is a plain Button with .contentShape so the
// entire non-button area navigates to the thread.
// ─────────────────────────────────────────────────────────────────────────

struct NoteRowView: View {
    let item: TimelineItem
    var contentTree: ContentTreeWire?
    var mentionProfiles: [String: MentionProfile] = [:]
    var eventCards: [String: ChirpEventCard] = [:]
    var timelineItems: [String: TimelineItem] = [:]
    var relationCounts: NoteRelationCounts? = nil
    let onLike: (String) -> Void
    /// NIP-57 — (eventID, authorPubkey, lnurl) → dispatch the zap. Optional
    /// so callers that don't surface zap (e.g. thread / profile views that
    /// have not yet been wired) can omit it. The actions row hides the zap
    /// button when this is `nil` OR `item.authorLnurl == nil`.
    var onZap: ((String, String, String) -> Void)? = nil

    @EnvironmentObject private var router: ChirpRouter
    @EnvironmentObject private var model: KernelModel

    /// Controls the inline reply sheet for this row.
    @State private var showReply = false
    /// Transient like-animation state.
    @State private var likeTapped = false

    /// ADR-0032 presentation-layer derivations of the raw `authorPubkey`
    /// hex. Kept as computed properties so the view body stays readable.
    private var authorDisplayLabel: String {
        model.profile(forPubkey: item.authorPubkey)?.display
            ?? eventCards[item.id]?.authorDisplayName
            ?? mentionProfiles[item.authorPubkey]?.display
            ?? item.authorPubkey.shortHex
    }

    private var authorAvatarInitials: String {
        let name = model.profile(forPubkey: item.authorPubkey)?.display
            ?? eventCards[item.id]?.authorDisplayName
        return (name ?? item.authorPubkey).displayInitials
    }

    private var authorAvatarColorHex: String {
        item.authorPubkey.pubkeyColorHex
    }

    var body: some View {
        Button {
            // For kind:6 reposts, the row represents the *inner* note (its
            // content + author/timestamp are the inner event's), so tapping
            // navigates to the inner note's thread, not the wrapper kind:6.
            // Rust pre-computes `navTargetId` so the view layer doesn't parse
            // protocol JSON (aim.md §6.9). For a kind:1 it equals `item.id`;
            // for a kind:6 it is the inner kind:1's id with a D1 fallback to
            // `item.id` when the embedded JSON is missing/malformed.
            router.push(.thread(eventID: item.navTargetId))
        } label: {
            VStack(alignment: .leading, spacing: 0) {
                rowContent
                NoteActionsRow(
                    item: item,
                    relationCounts: relationCounts,
                    onLike: onLike,
                    onZap: onZap,
                    likeTapped: $likeTapped,
                    showReply: $showReply
                )
                .padding(.top, 8)
                .padding(.leading, 52)

                Divider()
                    .padding(.top, 6)
                    .padding(.leading, 52)
            }
            .padding(.top, 12)
            .padding(.horizontal, 16)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .sheet(isPresented: $showReply) {
            ComposeView(replyToID: item.id, replyToShortID: item.id.shortHex)
        }
        .onAppear { model.claimVisibleNoteRelations(eventID: item.id) }
        .onDisappear { model.releaseVisibleNoteRelations(eventID: item.id) }
    }

    private var rowContent: some View {
        HStack(alignment: .top, spacing: 8) {
            avatarButton

            VStack(alignment: .leading, spacing: 4) {
                authorHeader
                noteContent
                relayChip
            }
        }
    }

    // ── Avatar (taps to profile) ──────────────────────────────────────────

    private var avatarButton: some View {
        Button {
            router.push(.profile(pubkey: item.authorPubkey))
        } label: {
            ChirpAvatar(
                pubkey: item.authorPubkey,
                url: item.authorPictureUrl,
                initials: authorAvatarInitials,
                colorHex: authorAvatarColorHex,
                size: 44
            )
        }
        .buttonStyle(.borderless)
        .accessibilityIdentifier("timeline-author-link")
    }

    // ── Author name + truncated pubkey + timestamp ────────────────────────

    private var authorHeader: some View {
        HStack(alignment: .firstTextBaseline, spacing: 4) {
            Text(authorDisplayLabel)
                .font(.headline)
                .foregroundStyle(.primary)
                .lineLimit(1)

            Spacer(minLength: 0)

            Text(item.createdAt.relativeTimeFromUnixSeconds)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    // ── Note content ──────────────────────────────────────────────────────

    private var noteContent: some View {
        let isRepost = item.isRepost
        let context = NoteRenderContext(
            mentionProfiles: mentionProfiles,
            eventCards: eventCards,
            timelineItems: timelineItems
        )
        let text = item.renderedContent
        let tree = context.contentTree(for: item, fallback: contentTree)
        return VStack(alignment: .leading, spacing: 4) {
            if isRepost {
                HStack(spacing: 3) {
                    Image(systemName: "arrow.2.squarepath")
                        .font(.system(size: 11, weight: .medium))
                    Text("Repost")
                        .font(.caption)
                }
                .foregroundStyle(.secondary)
            }
            if !text.isEmpty {
                NoteContentView(
                    content: text,
                    contentTree: tree,
                    renderContext: context,
                    font: .body
                )
                    .foregroundStyle(.primary)
            }
        }
        .padding(.top, 4)
    }

    // ── Relay-count chip ──────────────────────────────────────────────────

    @ViewBuilder
    private var relayChip: some View {
        if item.relayCount > 0 {
            HStack(spacing: 3) {
                Image(systemName: "antenna.radiowaves.left.and.right")
                    .font(.system(size: 10, weight: .medium))
                Text("\(item.relayCount)")
                    .font(.caption)
            }
            .foregroundStyle(.secondary)
            .padding(.top, 4)
        }
    }

}

// ─────────────────────────────────────────────────────────────────────────
// NoteActionsRow — reply / repost / like / zap action buttons.
// Kept in the same file for cohesion; small enough not to warrant a split.
// ─────────────────────────────────────────────────────────────────────────

struct NoteActionsRow: View {
    let item: TimelineItem
    let relationCounts: NoteRelationCounts?
    let onLike: (String) -> Void
    /// NIP-57 — invoked when the user taps the zap bolt. Hidden when this is
    /// `nil` (no zap wiring from the host) OR `item.authorLnurl == nil`
    /// (the author has no kind:0 lud16/lud06). Rust pre-computes
    /// `authorLnurl` so the row never parses metadata (thin-shell rule).
    var onZap: ((String, String, String) -> Void)? = nil
    @Binding var likeTapped: Bool
    @Binding var showReply: Bool

    var body: some View {
        HStack(spacing: 0) {
            actionButton(
                icon: "bubble.left",
                label: "Reply",
                count: relationCounts?.replies.value
            ) {
                showReply = true
            }

            Spacer()

            actionButton(
                icon: "arrow.2.squarepath",
                label: "Repost",
                count: relationCounts?.reposts.value
            ) {
                // Repost command not yet on kernel surface — no-op.
            }

            Spacer()

            likeButton

            Spacer()

            zapButton
        }
        .padding(.horizontal, 4)
    }

    // ── Zap (NIP-57) ─────────────────────────────────────────────────────

    /// Payment-styled bolt when the author has a kind:0 lightning address
    /// AND the host wired `onZap`; muted/static when either is missing.
    /// The disabled state still renders so the row layout stays stable
    /// regardless of whether the author has published lud16/lud06.
    @ViewBuilder
    private var zapButton: some View {
        if let onZap, let lnurl = item.authorLnurl {
            Button {
                onZap(item.id, item.authorPubkey, lnurl)
                UIImpactFeedbackGenerator(style: .soft).impactOccurred()
            } label: {
                actionLabel(icon: "bolt.fill", count: relationCounts?.zaps.value)
                    .foregroundStyle(ChirpColor.zap)
            }
            .buttonStyle(.borderless)
            .accessibilityLabel("Zap")
            .accessibilityIdentifier("note-zap-button")
        } else {
            // No lnurl OR no host wiring — keep the affordance visible so
            // row layout doesn't shift, but disabled and muted.
            Image(systemName: "bolt")
                .font(.system(size: 15, weight: .regular))
                .foregroundStyle(.secondary)
                .frame(minWidth: 44, minHeight: 32, alignment: .center)
                .accessibilityHidden(true)
        }
    }

    // ── Like with haptic feedback ────────────────────────────────────────

    private var likeButton: some View {
        Button {
            guard !likeTapped else { return }
            likeTapped = true
            onLike(item.id)
            UIImpactFeedbackGenerator(style: .soft).impactOccurred()
        } label: {
            actionLabel(icon: likeTapped ? "heart.fill" : "heart",
                        count: relationCounts?.reactions.value)
                .foregroundStyle(likeTapped ? ChirpColor.like : .secondary)
                .scaleEffect(likeTapped ? 1.15 : 1.0)
                .animation(.spring(response: 0.25, dampingFraction: 0.4), value: likeTapped)
        }
        .buttonStyle(.borderless)
    }

    // ── Generic action button factory ────────────────────────────────────

    @ViewBuilder
    private func actionButton(
        icon: String,
        label: String,
        count: UInt64?,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            actionLabel(icon: icon, count: count)
                .foregroundStyle(.secondary)
        }
        .buttonStyle(.borderless)
        .accessibilityLabel(label)
    }

    private func actionLabel(icon: String, count: UInt64?) -> some View {
        HStack(spacing: 4) {
            Image(systemName: icon)
                .font(.system(size: 15, weight: .regular))
            if let count, count > 0 {
                Text("\(count)")
                    .font(.caption)
            }
        }
        .frame(minWidth: 44, minHeight: 32, alignment: .center)
    }
}

// Previews omitted — KernelModel init requires the nmp_core FFI static lib
// which is not linked in the Xcode Preview host; previewing would crash.
// Test visually by running on simulator/device.
