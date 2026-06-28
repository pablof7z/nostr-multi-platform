import SwiftUI
import UIKit

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
    let item: NoteRowModel
    var contentTree: ContentTreeWire?
    var eventCards: [String: ChirpEventCard] = [:]
    var relationCounts: NoteRelationCounts? = nil
    let onLike: (String) -> Void
    /// NIP-18 — (eventID, authorPubkey) → dispatch kind:6 repost.
    var onRepost: ((String, String) -> Void)? = nil
    /// NIP-57 — (eventID, authorPubkey, lnurl) → dispatch the zap. Optional
    /// so callers that don't surface zap (e.g. thread / profile views that
    /// have not yet been wired) can omit it. The actions row hides the zap
    /// button when this is `nil` OR the keyed profile sidecar has no lnurl.
    var onZap: ((String, String, String) -> Void)? = nil

    @EnvironmentObject private var router: ChirpRouter
    @EnvironmentObject private var model: KernelModel

    /// Controls the inline reply sheet for this row.
    @State private var showReply = false
    @State private var showRelayProvenance = false
    /// Transient like-animation state.
    @State private var likeTapped = false

    /// ADR-0032 presentation-layer derivations of the raw `authorPubkey`
    /// hex. Kept as computed properties so the view body stays readable.
    private var authorDisplayLabel: String {
        Self.resolveAuthorLabel(
            profileDisplay: model.profile(forPubkey: item.authorPubkey)?.display,
            itemAuthorName: item.authorDisplayName,
            eventCardName: eventCards[item.id]?.authorDisplayName,
            shortHex: item.authorPubkey.shortHex)
    }

    /// Pure resolution of the author-label fallback chain, extracted so the
    /// precedence is unit-testable in isolation (a SwiftUI `View`'s private
    /// computed property and its `@EnvironmentObject` cannot be exercised
    /// from XCTest). The order is load-bearing:
    ///
    ///   1. `profileDisplay`  — `model.profile(forPubkey:)` reads the per-key
    ///      `keyedRefCache` (`refs.profile`), which now subsumes the former
    ///      claimed/resolved/mention rungs (ADR-0063 Lane E, #1671).
    ///   2. `itemAuthorName`  — baked into the typed event card at Rust build
    ///      time; claim-independent fallback that eliminates the 250–500ms
    ///      flicker gap (PR #823).
    ///   3. `eventCardName`   — NOFS gap-filler from the typed decoder.
    ///   4. `shortHex`        — last-resort raw-key abbreviation.
    ///
    /// ADR-0063 Lane E (#1671): the `mentionDisplay` rung is removed — it read
    /// the whole-map resolved-profiles dictionary that is now gone, and the
    /// keyed cache (rung 1) already covers it.
    ///
    /// Each rung is exercised by `ProfileNameFallbackTests`.
    static func resolveAuthorLabel(
        profileDisplay: String?,
        itemAuthorName: String? = nil,
        eventCardName: String?,
        shortHex: String
    ) -> String {
        profileDisplay
            ?? itemAuthorName
            ?? eventCardName
            ?? shortHex
    }

    private var authorPictureUrl: String? {
        model.profileCard(forPubkey: item.authorPubkey)?.pictureUrl
            ?? item.authorPictureUrl
    }

    var body: some View {
        Button {
            // For kind:6 reposts, the row represents the *inner* note (its
            // content + author/timestamp are the inner event's), so tapping
            // navigates to the inner note's thread, not the wrapper kind:6.
            // Rust emits the typed card id as the navigation target, so the
            // view layer never parses protocol JSON (aim.md §6.9).
            router.push(.thread(eventID: item.navTargetId))
        } label: {
            VStack(alignment: .leading, spacing: 0) {
                rowContent
                NoteActionsRow(
                    item: item,
                    authorLnurl: model.profileCard(forPubkey: item.authorPubkey)?.lnurl,
                    relationCounts: relationCounts,
                    onLike: onLike,
                    onRepost: onRepost,
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
            ComposeView(replyTo: ChirpReplyTarget(row: item))
        }
        .sheet(isPresented: $showRelayProvenance) {
            RelayProvenanceSheet(relays: item.relayProvenance)
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
            NostrAvatar(
                pubkey: item.authorPubkey,
                url: authorPictureUrl,
                size: 44
            )
            .equatable()
        }
        .buttonStyle(.borderless)
        .accessibilityIdentifier("timeline-author-link")
    }

    // ── Author name + truncated pubkey + timestamp ────────────────────────

    private var authorHeader: some View {
        HStack(alignment: .firstTextBaseline, spacing: 4) {
            // The author label is resolved across several kernel projections
            // (the PR #823 flicker fix in `resolveAuthorLabel`); the resolved
            // string is rendered through the shared `NostrProfileName`
            // component via its `displayName:` initializer.
            NostrProfileName(displayName: authorDisplayLabel)
                .accessibilityIdentifier("timeline-author-name")

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
            eventCards: eventCards
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
            Button {
                showRelayProvenance = true
            } label: {
                HStack(spacing: 3) {
                    Image(systemName: "antenna.radiowaves.left.and.right")
                        .font(.system(size: 10, weight: .medium))
                    Text("\(item.relayCount)")
                        .font(.caption)
                }
            }
            .buttonStyle(.borderless)
            .foregroundStyle(.secondary)
            .padding(.top, 4)
            .accessibilityLabel("Received from \(item.relayCount) relays")
        }
    }

}

private struct RelayProvenanceSheet: View {
    let relays: [String]
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                if relays.isEmpty {
                    Text("No relay provenance")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(relays, id: \.self) { relay in
                        Button {
                            UIPasteboard.general.string = relay
                        } label: {
                            Text(relay)
                                .font(.body.monospaced())
                                .textSelection(.enabled)
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
            .navigationTitle("Received from")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// NoteActionsRow — reply / repost / like / zap action buttons.
// Kept in the same file for cohesion; small enough not to warrant a split.
// ─────────────────────────────────────────────────────────────────────────

struct NoteActionsRow: View {
    let item: NoteRowModel
    let authorLnurl: String?
    let relationCounts: NoteRelationCounts?
    let onLike: (String) -> Void
    /// NIP-18 — (eventID, authorPubkey) → dispatch kind:6 repost.
    var onRepost: ((String, String) -> Void)? = nil
    /// NIP-57 — invoked when the user taps the zap bolt. Hidden when this is
    /// `nil` (no zap wiring from the host) OR `authorLnurl == nil` (the
    /// author has no kind:0 lud16/lud06). Rust pre-computes lnurl in the keyed
    /// profile sidecar; the row never parses metadata (thin-shell rule).
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
                onRepost?(item.id, item.authorPubkey)
                UIImpactFeedbackGenerator(style: .light).impactOccurred()
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
        if let onZap, let lnurl = authorLnurl {
            Button {
                onZap(item.id, item.authorPubkey, lnurl)
                UIImpactFeedbackGenerator(style: .soft).impactOccurred()
            } label: {
                actionLabel(icon: "bolt", count: relationCounts?.zaps.value)
                    .foregroundStyle(.secondary)
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
                .foregroundStyle(likeTapped ? ChirpColor.accent : .secondary)
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
