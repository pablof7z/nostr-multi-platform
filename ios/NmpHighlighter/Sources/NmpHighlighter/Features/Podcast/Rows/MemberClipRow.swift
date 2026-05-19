import SwiftUI

struct MemberClipRow: View {
    @Environment(HighlighterStore.self) private var app

    let highlight: HighlightRecord
    let state: TimelineRowState
    let onSeek: (Double) -> Void

    private var isExpanded: Bool {
        app.podcastPlayer.expandedClipId == highlight.eventId
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // -- Collapsed header (always visible) --
            Button {
                if let start = highlight.clipStartSeconds {
                    onSeek(start)
                }
                withAnimation(.easeInOut(duration: 0.2)) {
                    if isExpanded {
                        app.podcastPlayer.expandedClipId = nil
                    } else {
                        app.podcastPlayer.expandedClipId = highlight.eventId
                    }
                }
            } label: {
                HStack(alignment: .top, spacing: 14) {
                    Text(rangeLabel)
                        .font(.caption.weight(.medium).monospacedDigit())
                        .foregroundStyle(.secondary)
                        .frame(width: 48, alignment: .leading)

                    VStack(alignment: .leading, spacing: 8) {
                        HStack(alignment: .top, spacing: 10) {
                            AuthorAvatar(
                                pubkey: highlight.pubkey,
                                pictureURL: app.profileCache[highlight.pubkey]?.picture ?? "",
                                displayInitial: authorInitial,
                                size: 28
                            )

                            VStack(alignment: .leading, spacing: 2) {
                                Text(authorName)
                                    .font(.footnote.weight(.semibold))
                                    .foregroundStyle(.primary)
                                    .lineLimit(1)
                                Text(rangeLabel)
                                    .font(.caption2.monospacedDigit())
                                    .foregroundStyle(.secondary)
                            }

                            Spacer(minLength: 0)

                            Image(systemName: isExpanded ? "chevron.up" : "chevron.down")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }

                        if !highlight.quote.isEmpty {
                            HStack(alignment: .top, spacing: 8) {
                                Rectangle()
                                    .fill(Color.highlighterAccent)
                                    .frame(width: 2)
                                Text("\u{201C}\(highlight.quote)\u{201D}")
                                    .font(.system(.subheadline).italic())
                                    .foregroundStyle(.primary.opacity(0.9))
                                    .lineLimit(isExpanded ? nil : 3)
                                    .multilineTextAlignment(.leading)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                        }

                        if isExpanded && !highlight.note.isEmpty {
                            Text(highlight.note)
                                .font(.footnote)
                                .foregroundStyle(.secondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(
                    state == .active
                        ? Color(.separator).opacity(0.3)
                        : Color.clear
                )
                .opacity(state == .future ? 0.55 : 1.0)
            }
            .buttonStyle(.plain)

            // -- Thread expansion --
            if isExpanded {
                ClipThreadView(clipEventId: highlight.eventId)
                    .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .task(id: highlight.pubkey) {
            await app.requestProfile(pubkeyHex: highlight.pubkey)
        }
        .onChange(of: isExpanded) { _, expanded in
            guard expanded else { return }
            let id = highlight.eventId
            guard app.podcastPlayer.comments[id] == nil else { return }
            Task {
                let records = (try? await app.safeCore.getCommentsForReference(
                    tagName: "e",
                    tagValue: id,
                    limit: 200
                )) ?? []
                app.podcastPlayer.comments[id] = records
            }
        }
    }

    private var rangeLabel: String {
        let s = formatTimestamp(highlight.clipStartSeconds)
        let e = formatTimestamp(highlight.clipEndSeconds)
        if let s, let e { return "\(s)–\(e)" }
        if let s { return s }
        return "—"
    }

    private var authorName: String {
        let profile = app.profileCache[highlight.pubkey]
        if let dn = profile?.displayName, !dn.isEmpty { return dn }
        if let n = profile?.name, !n.isEmpty { return n }
        return String(highlight.pubkey.prefix(10))
    }

    private var authorInitial: String {
        authorName.first.map { String($0).uppercased() } ?? ""
    }
}

private func formatTimestamp(_ seconds: Double?) -> String? {
    guard let s = seconds, s >= 0 else { return nil }
    let total = Int(s.rounded())
    let h = total / 3600
    let m = (total % 3600) / 60
    let sec = total % 60
    if h > 0 { return String(format: "%d:%02d:%02d", h, m, sec) }
    return String(format: "%d:%02d", m, sec)
}
