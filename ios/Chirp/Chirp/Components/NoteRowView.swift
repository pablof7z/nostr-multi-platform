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
    let onLike: (String) -> Void

    @EnvironmentObject private var router: ChirpRouter

    /// Controls the inline reply sheet for this row.
    @State private var showReply = false
    /// Transient like-animation state.
    @State private var likeTapped = false

    var body: some View {
        Button {
            // For kind:6 reposts, the row represents the *inner* note (its
            // content + author/timestamp are the inner event's), so tapping
            // navigates to the inner note's thread, not the wrapper kind:6.
            // Fall back to the repost's own id if the inner event JSON is
            // missing or malformed (best-effort, D1).
            let destID = item.kind == 6
                ? (repostInnerEventId(item.content) ?? item.id)
                : item.id
            router.push(.thread(eventID: destID))
        } label: {
            VStack(alignment: .leading, spacing: 0) {
                rowContent
                NoteActionsRow(
                    item: item,
                    onLike: onLike,
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
            ComposeView(replyToID: item.id)
        }
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
                url: item.authorPictureUrl,
                initials: item.authorAvatarInitials,
                colorHex: item.authorAvatarColor,
                size: 44
            )
        }
        .buttonStyle(.borderless)
        .accessibilityIdentifier("timeline-author-link")
    }

    // ── Author name + truncated pubkey + timestamp ────────────────────────

    private var authorHeader: some View {
        HStack(alignment: .firstTextBaseline, spacing: 4) {
            Text(item.authorDisplay)
                .font(.headline)
                .foregroundStyle(.primary)
                .lineLimit(1)

            Text(shortPubkey(item.authorPubkey))
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
                .lineLimit(1)

            Spacer(minLength: 0)

            Text(item.createdAtDisplay)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    // ── Note content ──────────────────────────────────────────────────────

    private var noteContent: some View {
        let isRepost = item.kind == 6
        // For reposts, the visible text is the *inner* note's content (the
        // raw `content` field carries the embedded kind:1 event JSON per
        // NIP-18). When the inner JSON is missing or malformed, fall back to
        // an empty string — the "Repost" badge alone communicates state (D1).
        let text = isRepost ? (repostInnerText(item.content) ?? "") : item.content
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
                    // contentTree was computed against the wrapper event;
                    // it does not describe the inner note's text. Render the
                    // inner content as plain text (D1 best-effort) until the
                    // kernel emits a contentTree for the inner event.
                    contentTree: isRepost ? nil : contentTree,
                    mentionProfiles: mentionProfiles,
                    eventCards: eventCards,
                    timelineItems: timelineItems,
                    font: .body
                )
                    .foregroundStyle(.primary)
            }
        }
        .padding(.top, 4)
    }

    // ── Repost helpers (display-only JSON extraction) ─────────────────────
    //
    // Kind:6 reposts (NIP-18) carry the *full* reposted-event JSON in their
    // `content` field. These helpers pull the two fields the row needs for
    // rendering — they do not decide *what to do*, only *what to show* — so
    // they live in the view layer alongside the row that uses them. The
    // protocol-shaped signal (`item.kind`) comes from Rust; the helpers only
    // peek inside an already-validated kind:6 envelope.

    /// Returns the inner note's text content, or nil when the embedded event
    /// JSON is missing/malformed (some reposts ship an empty content field).
    private func repostInnerText(_ raw: String) -> String? {
        innerEventField(raw, field: "content")
    }

    /// Returns the inner note's event id (for thread navigation), or nil
    /// when the embedded JSON is missing/malformed.
    private func repostInnerEventId(_ raw: String) -> String? {
        innerEventField(raw, field: "id")
    }

    /// Generic helper: extract a top-level string field from an embedded
    /// Nostr event JSON object. Returns nil if `raw` is not a JSON object or
    /// the field is absent / not a string.
    private func innerEventField(_ raw: String, field: String) -> String? {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("{"),
              let data = trimmed.data(using: .utf8),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let value = json[field] as? String else { return nil }
        return value
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

    // ── Helpers ───────────────────────────────────────────────────────────

    /// "npub1abc…ef12" style truncation from hex pubkey.
    private func shortPubkey(_ hex: String) -> String {
        guard hex.count >= 12 else { return hex }
        return "\(hex.prefix(6))…\(hex.suffix(4))"
    }
}

// ─────────────────────────────────────────────────────────────────────────
// NoteActionsRow — reply / repost / like / zap action buttons.
// Kept in the same file for cohesion; small enough not to warrant a split.
// ─────────────────────────────────────────────────────────────────────────

struct NoteActionsRow: View {
    let item: TimelineItem
    let onLike: (String) -> Void
    @Binding var likeTapped: Bool
    @Binding var showReply: Bool

    var body: some View {
        HStack(spacing: 0) {
            actionButton(
                icon: "bubble.left",
                label: "Reply"
            ) {
                showReply = true
            }

            Spacer()

            actionButton(
                icon: "arrow.2.squarepath",
                label: "Repost"
            ) {
                // Repost command not yet on kernel surface — no-op.
            }

            Spacer()

            likeButton

            Spacer()

            actionButton(
                icon: "bolt",
                label: "Zap"
            ) {
                // Zap command not yet on kernel surface — no-op.
            }
        }
        .padding(.horizontal, 4)
    }

    // ── Like with haptic feedback ────────────────────────────────────────

    private var likeButton: some View {
        Button {
            guard !likeTapped else { return }
            likeTapped = true
            onLike(item.id)
            UIImpactFeedbackGenerator(style: .soft).impactOccurred()
        } label: {
            HStack(spacing: 5) {
                Image(systemName: likeTapped ? "heart.fill" : "heart")
                    .font(.system(size: 15, weight: .regular))
                    .foregroundStyle(likeTapped ? ChirpColor.like : .secondary)
                    .scaleEffect(likeTapped ? 1.35 : 1.0)
                    .animation(.spring(response: 0.25, dampingFraction: 0.4), value: likeTapped)
            }
            .frame(minWidth: 44, minHeight: 32, alignment: .center)
        }
        .buttonStyle(.borderless)
    }

    // ── Generic action button factory ────────────────────────────────────

    @ViewBuilder
    private func actionButton(
        icon: String,
        label: String,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: 15, weight: .regular))
                .foregroundStyle(.secondary)
                .frame(minWidth: 44, minHeight: 32, alignment: .center)
        }
        .buttonStyle(.borderless)
        .accessibilityLabel(label)
    }
}

// Previews omitted — KernelModel init requires the nmp_core FFI static lib
// which is not linked in the Xcode Preview host; previewing would crash.
// Test visually by running on simulator/device.
