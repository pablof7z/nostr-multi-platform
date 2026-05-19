import Kingfisher
import SwiftUI

/// Renders plain text that may contain `nostr:` URI mentions and event
/// references. Used by surfaces that don't run full markdown — profile
/// bios, room descriptions, chat messages, discussions. The article
/// reader (`MarkdownRenderer`) does its own pass and integrates the
/// inline components from this file directly.
///
/// Strategy:
///   1. Tokenise the input into a sequence of `.text("…")` runs and
///      `.entity(ref)` runs by scanning for `nostr:` URI prefixes.
///   2. Group consecutive runs into paragraphs split at event-ref
///      runs — mentions stay inline (concatenated into the surrounding
///      `Text`), event refs become block cards.
///   3. Each block renders the appropriate per-kind card (article,
///      note, highlight, profile-callout) by resolving the entity
///      against the local cache and falling back to a backfill REQ
///      when it isn't there yet.
struct NostrRichText: View {
    let content: String
    /// Base font for plain text + inline mentions. Defaults to body.
    var font: Font = .body
    /// Tint applied to inline mention chips.
    var accent: Color = .highlighterAccent
    var ink: Color = .highlighterInkStrong
    var muted: Color = .highlighterInkMuted

    @Environment(HighlighterStore.self) private var appStore

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                switch block {
                case .paragraph(let runs):
                    paragraph(runs)
                case .eventRef(let ref):
                    NostrEntityCard(entity: ref)
                }
            }
        }
    }

    // MARK: - Paragraph rendering

    @ViewBuilder
    private func paragraph(_ runs: [Run]) -> some View {
        // Concatenate runs into a single Text so wrapping behaves like
        // a normal paragraph. Mentions render with the cached display
        // name when available.
        runs.reduce(Text(""), { acc, run in
            switch run {
            case .text(let s):
                let a = (try? AttributedString(
                    markdown: s,
                    options: AttributedString.MarkdownParsingOptions(
                        interpretedSyntax: .inlineOnlyPreservingWhitespace
                    )
                )) ?? AttributedString(s)
                return acc + Text(a)
            case .entity(let ref):
                guard case .profile(let pubkey, _) = ref else {
                    // Event refs at this layer are guaranteed to be the
                    // first run of an `eventRef` block via `blocks`,
                    // so this case is unreachable in paragraphs.
                    return acc
                }
                let label = mentionLabel(for: pubkey)
                return acc + Text("@\(label)")
                    .foregroundStyle(accent)
                    .font(font.weight(.medium))
            }
        })
        .font(font)
        .foregroundStyle(ink)
        .fixedSize(horizontal: false, vertical: true)
    }

    private func mentionLabel(for pubkeyHex: String) -> String {
        let cached = appStore.profileCache[pubkeyHex]
        if let display = cached?.displayName, !display.isEmpty { return display }
        if let name = cached?.name, !name.isEmpty { return name }
        // Warm the cache so the next render swaps in a real name.
        Task { await appStore.requestProfile(pubkeyHex: pubkeyHex) }
        return String(pubkeyHex.prefix(8))
    }

    // MARK: - Tokenisation + blocking

    private var blocks: [Block] {
        var blocks: [Block] = []
        var currentRuns: [Run] = []
        for run in tokenise(content) {
            switch run {
            case .text, .entity(.profile):
                currentRuns.append(run)
            case .entity(let ref):
                if !currentRuns.isEmpty {
                    blocks.append(.paragraph(currentRuns))
                    currentRuns.removeAll()
                }
                blocks.append(.eventRef(ref))
            }
        }
        if !currentRuns.isEmpty {
            blocks.append(.paragraph(currentRuns))
        }
        return blocks
    }

    /// Walks `content`, emitting a sequence of `.text` and `.entity`
    /// runs by matching the canonical NIP-21 URI shape:
    /// `nostr:` followed by an HRP (`npub1`, `nprofile1`, `note1`,
    /// `nevent1`, `naddr1`) and a bech32 body.
    private func tokenise(_ s: String) -> [Run] {
        var out: [Run] = []
        var i = s.startIndex
        let scalars = s
        while i < scalars.endIndex {
            if let r = scanNostrURI(at: i, in: scalars) {
                if r.0.lowerBound > i {
                    let pre = String(scalars[i..<r.0.lowerBound])
                    out.append(.text(pre))
                }
                if let entity = decode(r.1) {
                    out.append(.entity(entity))
                } else {
                    // Fall back to literal text so a malformed URI still
                    // renders as something instead of vanishing.
                    out.append(.text(r.1))
                }
                i = r.0.upperBound
            } else {
                out.append(.text(String(scalars[i...])))
                break
            }
        }
        return out
    }

    /// Find the next `nostr:<hrp>1<bech32>` token. Returns the matched
    /// range and the URI string (without the leading `nostr:`).
    private func scanNostrURI(
        at start: String.Index,
        in s: String
    ) -> (Range<String.Index>, String)? {
        guard let prefixRange = s.range(of: "nostr:", options: [.literal, .caseInsensitive], range: start..<s.endIndex) else {
            return nil
        }
        let bodyStart = prefixRange.upperBound
        // Match the bech32 body — bech32 alphabet is [a-z0-9] (no
        // capital, no '0', '1' is allowed because '1' is the separator).
        // We accept lowercase ascii alphanumerics greedily.
        var end = bodyStart
        while end < s.endIndex, isBech32Char(s[end]) {
            end = s.index(after: end)
        }
        if end == bodyStart { return nil }
        let body = String(s[bodyStart..<end])
        // Only count it as an entity URI if it starts with one of the
        // recognised HRPs followed by `1` (the bech32 separator).
        let lower = body.lowercased()
        guard lower.hasPrefix("npub1")
            || lower.hasPrefix("nprofile1")
            || lower.hasPrefix("note1")
            || lower.hasPrefix("nevent1")
            || lower.hasPrefix("naddr1")
        else {
            return nil
        }
        return (prefixRange.lowerBound..<end, body)
    }

    private func isBech32Char(_ c: Character) -> Bool {
        guard let scalar = c.unicodeScalars.first, c.unicodeScalars.count == 1 else { return false }
        let v = scalar.value
        // Lowercase ASCII letters + digits (bech32 alphabet is a subset
        // but accepting the full alphanumeric set here is fine — the
        // FromBech32 decoder rejects invalid inputs).
        return (0x30...0x39).contains(v) || (0x61...0x7A).contains(v)
    }

    private func decode(_ raw: String) -> NostrEntityRef? {
        try? appStore.core.decodeNostrEntity(input: raw)
    }

    /// Pull every `nostr:` event-reference entity (`note1…`, `nevent1…`,
    /// `naddr1…`) out of `content`, deduped by reference key, in the
    /// order they first appeared. Profile mentions (`npub1…`,
    /// `nprofile1…`) are excluded — those render inline via the main
    /// `body`. Used by the article reader to render a "Referenced"
    /// section after the article body without doing a full mid-stream
    /// markdown refactor.
    static func extractEventRefs(
        from content: String,
        using core: HighlighterCore
    ) -> [NostrEntityRef] {
        var seen: Set<String> = []
        var out: [NostrEntityRef] = []
        for run in tokeniseWithDecoder(content, decoder: { try? core.decodeNostrEntity(input: $0) }) {
            guard case .entity(let ref) = run else { continue }
            switch ref {
            case .profile:
                continue
            case .event(let id, _, _, _):
                if seen.insert("e:\(id)").inserted { out.append(ref) }
            case .address(let kind, let pk, let d, _):
                if seen.insert("a:\(kind):\(pk):\(d)").inserted { out.append(ref) }
            }
        }
        return out
    }

    /// Pure tokeniser variant the static `extractEventRefs` can call
    /// without needing an `appStore` environment. Mirrors the instance
    /// `tokenise` but takes the decoder as a parameter.
    private static func tokeniseWithDecoder(
        _ s: String,
        decoder: (String) -> NostrEntityRef?
    ) -> [Run] {
        var out: [Run] = []
        var i = s.startIndex
        while i < s.endIndex {
            guard let prefixRange = s.range(of: "nostr:", options: [.literal, .caseInsensitive], range: i..<s.endIndex) else {
                out.append(.text(String(s[i...])))
                break
            }
            let bodyStart = prefixRange.upperBound
            var end = bodyStart
            while end < s.endIndex {
                let c = s[end]
                guard let scalar = c.unicodeScalars.first, c.unicodeScalars.count == 1 else { break }
                let v = scalar.value
                if (0x30...0x39).contains(v) || (0x61...0x7A).contains(v) {
                    end = s.index(after: end)
                } else { break }
            }
            if end == bodyStart { i = prefixRange.upperBound; continue }
            let body = String(s[bodyStart..<end])
            let lower = body.lowercased()
            let isEntity = lower.hasPrefix("npub1") || lower.hasPrefix("nprofile1")
                || lower.hasPrefix("note1") || lower.hasPrefix("nevent1") || lower.hasPrefix("naddr1")
            if !isEntity { i = prefixRange.upperBound; continue }
            if prefixRange.lowerBound > i { out.append(.text(String(s[i..<prefixRange.lowerBound]))) }
            if let entity = decoder(body) { out.append(.entity(entity)) } else { out.append(.text(body)) }
            i = end
        }
        return out
    }

    // MARK: - Run / Block models

    private enum Run {
        case text(String)
        case entity(NostrEntityRef)
    }

    private enum Block {
        case paragraph([Run])
        case eventRef(NostrEntityRef)
    }
}