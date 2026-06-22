struct NoteRenderContext: Equatable, Sendable {
    let eventCards: [String: ChirpEventCard]
    let timelineItems: [String: TimelineItem]

    static let empty = NoteRenderContext(
        eventCards: [:],
        timelineItems: [:]
    )

    // ADR-0063 Lane E (#1671): inline mention labels are NO LONGER carried in
    // the render context. A whole-map profile dictionary threaded through every
    // row makes a single kind:0 update re-render the whole list (and broadcasts
    // via `@Published`). Mentions now read the per-key `KeyedRefCache` at render
    // time in `NoteContentView`, observed per-key so only the notes mentioning
    // the updated pubkey re-render.
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

func shortEntity(_ value: String) -> String {
    guard value.count > 12 else { return value }
    return "\(value.prefix(8))…\(value.suffix(4))"
}

extension ContentTreeWire {
    /// Hex pubkeys of every `nostr:npub…` / `nprofile…` profile mention in this
    /// content tree, in stable arena order, de-duplicated.
    ///
    /// F-CR-00 claim-only invariant: a mention is an author-displaying surface,
    /// so the rendering view must claim each mentioned pubkey's kind:0 (mirror
    /// of `NostrAvatar`). Mentions render as inline `Text` runs inside a single
    /// concatenated `Text` (no per-mention SwiftUI view with its own lifecycle),
    /// so the claim is hoisted to the host view (`NoteContentView`) keyed off
    /// this list.
    var mentionPubkeys: [String] {
        var seen = Set<String>()
        var out: [String] = []
        for node in nodes {
            if case .mention(let uri) = node, uri.kind == .profile {
                let pk = uri.primaryId
                if !pk.isEmpty, seen.insert(pk).inserted {
                    out.append(pk)
                }
            }
        }
        return out
    }
}
