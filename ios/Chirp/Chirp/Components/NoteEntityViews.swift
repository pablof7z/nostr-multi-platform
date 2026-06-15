struct NoteRenderContext: Equatable, Sendable {
    let mentionProfiles: [String: MentionProfile]
    let eventCards: [String: ChirpEventCard]
    let timelineItems: [String: TimelineItem]

    static let empty = NoteRenderContext(
        mentionProfiles: [:],
        eventCards: [:],
        timelineItems: [:]
    )

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
