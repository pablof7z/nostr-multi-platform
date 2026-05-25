import SwiftUI

enum NoteContentGroup: Equatable {
    case inline([UInt32])
    case media(urls: [String], kind: MediaKind)
    case eventRef(WireNostrUri)
}

func noteContentGroups(_ tree: ContentTreeWire) -> [NoteContentGroup] {
    var groups: [NoteContentGroup] = []
    var run: [UInt32] = []

    func flush() {
        if !run.isEmpty {
            groups.append(.inline(run))
            run = []
        }
    }

    func appendInlineChildren(_ children: [UInt32]) {
        let startCount = run.count
        for child in children {
            guard let childNode = noteContentNode(child, in: tree) else { continue }
            if case .eventRef(let uri) = childNode {
                flush()
                groups.append(.eventRef(uri))
            } else {
                run.append(child)
            }
        }
        if run.count > startCount {
            run.append(UInt32.max)
        }
    }

    for root in tree.roots {
        guard let node = noteContentNode(root, in: tree) else { continue }
        switch node {
        case .media(let urls, let kind):
            flush()
            groups.append(.media(urls: urls, kind: kind))
        case .eventRef(let uri):
            flush()
            groups.append(.eventRef(uri))
        case .paragraph(let children), .heading(_, let children):
            appendInlineChildren(children)
        default:
            run.append(root)
        }
    }
    flush()
    return groups
}

private func noteContentNode(_ index: UInt32, in tree: ContentTreeWire) -> ContentWireNode? {
    let i = Int(index)
    guard i >= 0, i < tree.nodes.count else { return nil }
    return tree.nodes[i]
}

struct NoteRenderContext: Equatable, Sendable {
    let mentionProfiles: [String: MentionProfile]
    let eventCards: [String: ChirpEventCard]
    let timelineItems: [String: TimelineItem]
    let embedDepth: Int

    static let empty = NoteRenderContext(
        mentionProfiles: [:],
        eventCards: [:],
        timelineItems: [:],
        embedDepth: 0
    )

    func child() -> NoteRenderContext {
        NoteRenderContext(
            mentionProfiles: mentionProfiles,
            eventCards: eventCards,
            timelineItems: timelineItems,
            embedDepth: embedDepth + 1
        )
    }

    func mentionLabel(for pubkey: String) -> String {
        mentionProfiles[pubkey]?.display ?? shortEntity(pubkey)
    }

    // V-31 — wrong-algorithm fallback removed. The previous code computed
    // `colorHex: "#" + String(pubkey.prefix(6))` and `initials:
    // pubkey.prefix(2)` in Swift, which never matched the canonical djb2
    // avatar color or kind:0-derived initials the kernel emits in
    // `mention_profiles` / `TimelineItem`. Now we surface a safe,
    // predictable placeholder (`"?"` / neutral grey) for pubkeys that have
    // no loaded profile — the only state in which the kernel-owned map
    // would not carry an entry. Display is the kernel-owned `shortEntity`
    // (npub… abbreviation), which stays stable across re-renders.
    func authorProfile(for pubkey: String) -> MentionProfile {
        mentionProfiles[pubkey] ?? MentionProfile(
            display: shortEntity(pubkey),
            pictureUrl: nil,
            initials: "?",
            colorHex: "888888"
        )
    }

    func contentTree(for item: TimelineItem, fallback: ContentTreeWire?) -> ContentTreeWire? {
        if item.isRepost {
            return eventCards[item.id]?.contentTree
                ?? eventCards[item.navTargetId]?.contentTree
                ?? fallback
        }
        return fallback ?? eventCards[item.id]?.contentTree
    }
}

extension TimelineItem {
    var renderedContent: String {
        isRepost ? repostInnerContent : content
    }
}

struct EmbeddedNostrEventCard: View {
    let uri: WireNostrUri
    let context: NoteRenderContext

    @EnvironmentObject private var router: ChirpRouter

    var body: some View {
        if context.embedDepth > 0 {
            tappableCollapsedCard(title: "Quoted post", detail: uri.primaryId, systemImage: "arrow.up.left.and.arrow.down.right")
        } else if let card = context.eventCards[uri.primaryId] {
            embeddedCard(card)
        } else if let item = context.timelineItems[uri.primaryId] {
            itemCard(item)
        } else {
            tappableCollapsedCard(title: "Quoted post", detail: uri.primaryId, systemImage: "quote.bubble")
        }
    }

    private func embeddedCard(_ card: ChirpEventCard) -> some View {
        Button {
            router.push(.thread(eventID: card.id))
        } label: {
            VStack(alignment: .leading, spacing: 8) {
                eventHeader(
                    eventID: card.id,
                    pubkey: card.authorPubkey,
                    kind: card.kind,
                    createdAt: card.createdAtDisplay
                )
                NoteContentView(
                    content: card.content,
                    contentTree: card.contentTree,
                    renderContext: context.child(),
                    font: .callout
                )
                .lineLimit(8)
            }
            .embeddedCardStyle()
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Open quoted event")
    }

    private func itemCard(_ item: TimelineItem) -> some View {
        Button {
            router.push(.thread(eventID: item.id))
        } label: {
            VStack(alignment: .leading, spacing: 8) {
                eventHeader(
                    eventID: item.id,
                    pubkey: item.authorPubkey,
                    kind: 1,
                    createdAt: item.createdAtDisplay
                )
                Text(item.contentPreview.isEmpty ? item.content : item.contentPreview)
                    .font(.callout)
                    .foregroundStyle(.primary)
                    .lineLimit(8)
            }
            .embeddedCardStyle()
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Open quoted event")
    }

    private func eventHeader(eventID: String, pubkey: String, kind: UInt32, createdAt: String) -> some View {
        let profile = context.authorProfile(for: pubkey)
        return HStack(spacing: 8) {
            ChirpAvatar(
                url: profile.pictureUrl,
                initials: profile.initials,
                colorHex: profile.colorHex,
                size: 26
            )
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 4) {
                    Text("@\(profile.display)")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.primary)
                        .lineLimit(1)
                    Text("kind \(kind)")
                        .font(.caption2.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                Text("\(createdAt) · \(shortEntity(eventID))")
                    .font(.caption2.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
            Image(systemName: "chevron.right")
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.tertiary)
        }
    }

    private func tappableCollapsedCard(title: String, detail: String, systemImage: String) -> some View {
        Button {
            router.push(.thread(eventID: detail))
        } label: {
            HStack(alignment: .center, spacing: 10) {
                Image(systemName: systemImage)
                    .font(.callout.weight(.semibold))
                    .foregroundStyle(ChirpColor.link)
                    .frame(width: 24, height: 24)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.primary)
                    Text(shortEntity(detail))
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.tertiary)
            }
            .embeddedCardStyle()
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Open quoted post")
    }

    private func collapsedCard(title: String, detail: String, systemImage: String) -> some View {
        HStack(alignment: .center, spacing: 10) {
            Image(systemName: systemImage)
                .font(.callout.weight(.semibold))
                .foregroundStyle(ChirpColor.link)
                .frame(width: 24, height: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.primary)
                Text(shortEntity(detail))
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
        .embeddedCardStyle()
    }

}

private extension View {
    func embeddedCardStyle() -> some View {
        self
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(ChirpColor.surface.opacity(0.75), in: RoundedRectangle(cornerRadius: 8))
            .overlay(
                RoundedRectangle(cornerRadius: 8)
                    .stroke(ChirpColor.hairline.opacity(0.55), lineWidth: 0.5)
            )
    }
}

func shortEntity(_ value: String) -> String {
    guard value.count > 12 else { return value }
    return "\(value.prefix(8))…\(value.suffix(4))"
}
