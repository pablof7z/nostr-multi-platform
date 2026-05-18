import Kingfisher
import SwiftUI

/// Modal presented when a card on the explorer is tapped. Starts at
/// `.medium` with the hero, description, and Join button; "Peek inside"
/// expands the sheet to `.large` and streams the room's recent artifacts
/// inline — no dismissal, no navigation. "Open full room" is available
/// from the expanded state for when the user wants the real deal.
struct RoomPreviewSheet: View {
    let room: CommunitySummary
    let onJoin: () -> Void
    var onOpenRoom: (() -> Void)? = nil

    @Environment(HighlighterStore.self) private var appStore
    @Environment(\.dismiss) private var dismiss

    @State private var detent: PresentationDetent = .medium
    @State private var roomStore: RoomStore?

    private var alreadyJoined: Bool {
        appStore.joinedCommunities.contains(where: { $0.id == room.id })
    }

    private var isExpanded: Bool { detent == .large }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                heroBackdrop

                VStack(alignment: .leading, spacing: 10) {
                    Text(room.name)
                        .font(.system(.title2, design: .default).weight(.semibold))
                        .foregroundStyle(Color.highlighterInkStrong)

                    meta

                    if !room.about.isEmpty {
                        NostrRichText(content: room.about, font: .body)
                            .padding(.top, 4)
                    }
                }
                .padding(.horizontal, 20)

                if isExpanded {
                    insideSection
                        .padding(.horizontal, 20)
                        .transition(.opacity.combined(with: .move(edge: .bottom)))
                }

                Spacer(minLength: 12)

                actionStack
                    .padding(.horizontal, 20)
                    .padding(.bottom, 20)
            }
            .animation(.easeInOut(duration: 0.25), value: isExpanded)
        }
        .background(Color.highlighterPaper.ignoresSafeArea())
        .presentationDetents([.medium, .large], selection: $detent)
        .presentationDragIndicator(.visible)
        .onChange(of: isExpanded) { _, expanded in
            if expanded { startRoomStoreIfNeeded() }
        }
        .onDisappear {
            roomStore?.stop()
        }
    }

    // MARK: - Sections

    private var heroBackdrop: some View {
        ZStack(alignment: .bottomLeading) {
            if let url = URL(string: room.picture), !room.picture.isEmpty {
                KFImage(url)
                    .placeholder { coverFallback }
                    .fade(duration: 0.2)
                    .resizable()
                    .scaledToFill()
            } else {
                coverFallback
            }

            LinearGradient(
                colors: [
                    .black.opacity(0.0),
                    .black.opacity(0.35),
                ],
                startPoint: .top,
                endPoint: .bottom
            )
        }
        .frame(height: 220)
        .frame(maxWidth: .infinity)
        .clipped()
    }

    private var meta: some View {
        HStack(spacing: 10) {
            accessBadge
            if let count = room.memberCount, count > 0 {
                Label {
                    Text(count == 1 ? "1 member" : "\(count) members")
                } icon: {
                    Image(systemName: "person.2")
                }
                .labelStyle(.titleAndIcon)
                .font(.caption.weight(.medium))
                .foregroundStyle(Color.highlighterInkMuted)
            }
        }
    }

    private var accessBadge: some View {
        let isOpen = room.access == "open"
        return HStack(spacing: 4) {
            Image(systemName: isOpen ? "lock.open" : "lock")
                .font(.caption2.weight(.semibold))
            Text(isOpen ? "Open" : "Closed")
                .font(.caption.weight(.semibold))
        }
        .foregroundStyle(Color.highlighterInkStrong)
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(
            Capsule().fill(
                isOpen ? Color.highlighterTintPale : Color.highlighterRule.opacity(0.45)
            )
        )
    }

    @ViewBuilder
    private var insideSection: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("RECENT")
                .font(.caption.weight(.semibold))
                .tracking(1.2)
                .foregroundStyle(Color.highlighterInkMuted)

            if let store = roomStore, !store.artifacts.isEmpty {
                VStack(spacing: 0) {
                    ForEach(Array(store.artifacts.prefix(8)), id: \.shareEventId) { artifact in
                        InsideArtifactRow(artifact: artifact)
                        if artifact.shareEventId != store.artifacts.prefix(8).last?.shareEventId {
                            Divider().overlay(Color.highlighterRule)
                        }
                    }
                }
                .background(
                    RoundedRectangle(cornerRadius: 14)
                        .stroke(Color.highlighterRule, lineWidth: 1)
                )
            } else if roomStore?.isLoading == true || roomStore == nil {
                HStack(spacing: 10) {
                    ProgressView().controlSize(.small)
                    Text("Pulling recent content…")
                        .font(.subheadline)
                        .foregroundStyle(Color.highlighterInkMuted)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.vertical, 18)
            } else {
                Text("Nothing shared here yet.")
                    .font(.subheadline)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 12)
            }
        }
    }

    @ViewBuilder
    private var actionStack: some View {
        if alreadyJoined {
            Button {
                if let onOpenRoom {
                    onOpenRoom()
                } else {
                    dismiss()
                }
            } label: {
                Text("Open room")
                    .font(.headline)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 14)
                    .background(
                        RoundedRectangle(cornerRadius: 14)
                            .fill(Color.highlighterAccent)
                    )
                    .foregroundStyle(.white)
            }
            .buttonStyle(.plain)
        } else {
            VStack(spacing: 10) {
                Button(action: onJoin) {
                    Text(room.access == "closed" ? "Request to join" : "Join room")
                        .font(.headline)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 14)
                        .background(
                            RoundedRectangle(cornerRadius: 14)
                                .fill(Color.highlighterAccent)
                        )
                        .foregroundStyle(.white)
                }
                .buttonStyle(.plain)

                if room.access == "open" {
                    if isExpanded {
                        Button {
                            if let onOpenRoom {
                                onOpenRoom()
                            } else {
                                dismiss()
                            }
                        } label: {
                            Text("Open full room")
                                .font(.subheadline.weight(.medium))
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 12)
                                .foregroundStyle(Color.highlighterInkStrong)
                                .overlay(
                                    RoundedRectangle(cornerRadius: 14)
                                        .stroke(Color.highlighterRule, lineWidth: 1)
                                )
                        }
                        .buttonStyle(.plain)
                    } else {
                        Button {
                            detent = .large
                        } label: {
                            Text("Peek inside")
                                .font(.subheadline.weight(.medium))
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 12)
                                .foregroundStyle(Color.highlighterInkStrong)
                                .overlay(
                                    RoundedRectangle(cornerRadius: 14)
                                        .stroke(Color.highlighterRule, lineWidth: 1)
                                )
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
    }

    private var coverFallback: some View {
        LinearGradient(
            colors: [
                Color.highlighterAccent.opacity(0.72),
                Color.highlighterAccent.opacity(0.36),
            ],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
    }

    // MARK: - Private

    private func startRoomStoreIfNeeded() {
        guard roomStore == nil else { return }
        let store = RoomStore()
        roomStore = store
        Task {
            await store.start(
                groupId: room.id,
                core: appStore.safeCore,
                bridge: appStore.eventBridge
            )
        }
    }
}

/// Compact artifact row used inside the peek sheet. Just the essentials —
/// title, source, author. Full detail is a tap-through on the room page.
private struct InsideArtifactRow: View {
    let artifact: ArtifactRecord

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            cover
                .frame(width: 44, height: 44)
                .clipShape(RoundedRectangle(cornerRadius: 8))

            VStack(alignment: .leading, spacing: 2) {
                Text(displayTitle)
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(2)
                    .multilineTextAlignment(.leading)

                if !artifact.preview.author.isEmpty {
                    Text(artifact.preview.author)
                        .font(.caption)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(1)
                } else if !artifact.preview.domain.isEmpty {
                    Text(artifact.preview.domain)
                        .font(.caption)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(1)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
    }

    private var displayTitle: String {
        let t = artifact.preview.title.trimmingCharacters(in: .whitespacesAndNewlines)
        return t.isEmpty ? "Untitled" : t
    }

    @ViewBuilder
    private var cover: some View {
        if let url = URL(string: artifact.preview.image), !artifact.preview.image.isEmpty {
            KFImage(url)
                .placeholder { coverFallback }
                .fade(duration: 0.15)
                .resizable()
                .scaledToFill()
        } else {
            coverFallback
        }
    }

    private var coverFallback: some View {
        LinearGradient(
            colors: [
                Color.highlighterAccent.opacity(0.32),
                Color.highlighterAccent.opacity(0.12),
            ],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
    }
}
