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

// Shorthand to peek at the variant without binding.
private extension NostrEntityRef {
    var isProfile: Bool {
        if case .profile = self { return true }
        return false
    }
}

private extension Optional where Wrapped == NostrEntityRef {
    var asProfile: (pubkeyHex: String, relays: [String])? {
        guard case .profile(let pubkey, let relays) = self else { return nil }
        return (pubkey, relays)
    }
}

// MARK: - Card

/// Block-level card for `nevent1…` / `naddr1…` references. Resolves
/// against the local nostrdb first, fires a backfill REQ when cold,
/// and re-resolves on the next render after the event lands. Per-kind
/// rendering swap-in is handled inline.
struct NostrEntityCard: View {
    let entity: NostrEntityRef

    @Environment(HighlighterStore.self) private var appStore
    @State private var resolved: NostrEntityEvent?
    @State private var attempted = false

    var body: some View {
        Group {
            if let resolved {
                resolvedCard(resolved)
            } else {
                placeholder
            }
        }
        .task(id: cacheKey) {
            await load()
        }
    }

    private var cacheKey: String {
        switch entity {
        case .profile(let pk, _): return "p:\(pk)"
        case .event(let id, _, _, _): return "e:\(id)"
        case .address(let kind, let pk, let d, _): return "a:\(kind):\(pk):\(d)"
        }
    }

    @ViewBuilder
    private func resolvedCard(_ event: NostrEntityEvent) -> some View {
        switch Int(event.kind) {
        case 30023: ArticleEntityCard(event: event)
        case 1: NoteEntityCard(event: event)
        case 9802: HighlightEntityCard(event: event)
        case 0: ProfileCalloutCard(event: event)
        default: GenericEntityCard(event: event)
        }
    }

    private var placeholder: some View {
        HStack(spacing: 10) {
            ProgressView().controlSize(.small)
            Text(entityLabel)
                .font(.caption)
                .foregroundStyle(Color.highlighterInkMuted)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 0)
        }
        .padding(12)
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.highlighterRule, lineWidth: 1)
        )
    }

    private var entityLabel: String {
        switch entity {
        case .profile(let pk, _): return "Profile · \(pk.prefix(12))…"
        case .event(let id, _, _, let kind):
            if let k = kind { return "Event kind \(k) · \(id.prefix(12))…" }
            return "Event · \(id.prefix(12))…"
        case .address(let kind, _, let d, _): return "Kind \(kind) · \(d)"
        }
    }

    private func load() async {
        if let cached = try? await appStore.safeCore.resolveNostrEntity(entity) {
            await MainActor.run { resolved = cached }
        }
        if resolved == nil && !attempted {
            attempted = true
            try? await appStore.safeCore.subscribeNostrEntity(entity)
            // Brief poll-back so a fast arrival populates without a view
            // re-create. The subscription terminates on EOSE so this is
            // bounded.
            try? await Task.sleep(for: .milliseconds(800))
            if let cached = try? await appStore.safeCore.resolveNostrEntity(entity) {
                await MainActor.run { resolved = cached }
            }
        }
    }
}

// MARK: - Per-kind cards

/// kind:30023 — long-form article. Compact magazine card.
private struct ArticleEntityCard: View {
    let event: NostrEntityEvent
    @Environment(HighlighterStore.self) private var appStore
    @State private var profile: ProfileMetadata?

    var body: some View {
        let tags = parseTags(event.tagsJson)
        let title = tagValue(tags, "title")
        let image = tagValue(tags, "image")
        let summary = tagValue(tags, "summary")
        let dTag = tagValue(tags, "d")
        let target = ArticleReaderTarget(
            pubkey: event.pubkeyHex,
            dTag: dTag,
            seed: nil
        )
        return NavigationLink(value: target) {
            HStack(alignment: .top, spacing: 12) {
                if let url = URL(string: image), !image.isEmpty {
                    KFImage(url)
                        .resizable()
                        .scaledToFill()
                        .frame(width: 88, height: 88)
                        .clipShape(RoundedRectangle(cornerRadius: 8))
                } else {
                    RoundedRectangle(cornerRadius: 8)
                        .fill(Color.highlighterTintPale)
                        .frame(width: 88, height: 88)
                }
                VStack(alignment: .leading, spacing: 4) {
                    Text(title.isEmpty ? "Untitled" : title)
                        .font(.system(.headline, design: .default))
                        .foregroundStyle(Color.highlighterInkStrong)
                        .lineLimit(2)
                    if !summary.isEmpty {
                        Text(summary)
                            .font(.caption)
                            .foregroundStyle(Color.highlighterInkMuted)
                            .lineLimit(2)
                    }
                    let authorName = profile?.displayName ?? profile?.name ?? String(event.pubkeyHex.prefix(8))
                    HStack(spacing: 6) {
                        AuthorAvatar(
                            pubkey: event.pubkeyHex,
                            pictureURL: profile?.picture ?? "",
                            displayInitial: authorName.prefix(1).description.uppercased(),
                            size: 16,
                            ringWidth: 0
                        )
                        Text(authorName.uppercased())
                            .font(.caption2.weight(.bold))
                            .tracking(0.6)
                            .foregroundStyle(Color.highlighterInkMuted)
                            .lineLimit(1)
                    }
                    .padding(.top, 2)
                }
                Spacer(minLength: 0)
            }
            .padding(12)
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .stroke(Color.highlighterRule, lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
        .task {
            profile = appStore.profileCache[event.pubkeyHex]
            if profile == nil {
                await appStore.requestProfile(pubkeyHex: event.pubkeyHex)
                profile = appStore.profileCache[event.pubkeyHex]
            }
        }
    }
}

/// kind:1 — short note. Tweet-like card with author header + content.
private struct NoteEntityCard: View {
    let event: NostrEntityEvent
    @Environment(HighlighterStore.self) private var appStore
    @State private var profile: ProfileMetadata?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                AuthorAvatar(
                    pubkey: event.pubkeyHex,
                    pictureURL: profile?.picture ?? "",
                    displayInitial: (profile?.displayName ?? profile?.name ?? event.pubkeyHex).prefix(1).description,
                    size: 26,
                    ringWidth: 0
                )
                Text(profile?.displayName ?? profile?.name ?? String(event.pubkeyHex.prefix(8)))
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkStrong)
                Spacer(minLength: 0)
                Text(relativeDate(event.createdAt))
                    .font(.caption)
                    .foregroundStyle(Color.highlighterInkMuted)
            }
            Text(event.content)
                .font(.body)
                .foregroundStyle(Color.highlighterInkStrong)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(12)
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.highlighterRule, lineWidth: 1)
        )
        .task {
            profile = appStore.profileCache[event.pubkeyHex]
            if profile == nil {
                await appStore.requestProfile(pubkeyHex: event.pubkeyHex)
                profile = appStore.profileCache[event.pubkeyHex]
            }
        }
    }
}

/// kind:9802 — NIP-84 highlight. Pull-quote treatment.
private struct HighlightEntityCard: View {
    let event: NostrEntityEvent
    @Environment(HighlighterStore.self) private var appStore
    @State private var profile: ProfileMetadata?

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            Rectangle()
                .fill(Color.highlighterAccent)
                .frame(width: 3)
                .frame(maxHeight: .infinity)
            VStack(alignment: .leading, spacing: 8) {
                Text(event.content)
                    .font(.system(.body, design: .default).italic())
                    .foregroundStyle(Color.highlighterInkStrong)
                Text("— \(profile?.displayName ?? profile?.name ?? String(event.pubkeyHex.prefix(8)))")
                    .font(.caption)
                    .foregroundStyle(Color.highlighterInkMuted)
            }
        }
        .padding(.vertical, 10)
        .padding(.horizontal, 4)
        .task {
            profile = appStore.profileCache[event.pubkeyHex]
            if profile == nil {
                await appStore.requestProfile(pubkeyHex: event.pubkeyHex)
                profile = appStore.profileCache[event.pubkeyHex]
            }
        }
    }
}

/// kind:0 — profile metadata. Compact callout.
private struct ProfileCalloutCard: View {
    let event: NostrEntityEvent

    var body: some View {
        // The content is JSON; we let the upstream profileCache supply
        // the parsed data via NavigationLink. Render the avatar +
        // name from the cache if present.
        ProfileCalloutFromCache(pubkey: event.pubkeyHex)
    }
}

private struct ProfileCalloutFromCache: View {
    let pubkey: String
    @Environment(HighlighterStore.self) private var appStore
    @State private var profile: ProfileMetadata?

    var body: some View {
        NavigationLink(value: ProfileDestination.pubkey(pubkey)) {
            HStack(spacing: 10) {
                AuthorAvatar(
                    pubkey: pubkey,
                    pictureURL: profile?.picture ?? "",
                    displayInitial: (profile?.displayName ?? profile?.name ?? pubkey).prefix(1).description,
                    size: 36,
                    ringWidth: 0
                )
                VStack(alignment: .leading, spacing: 2) {
                    Text(profile?.displayName ?? profile?.name ?? String(pubkey.prefix(8)))
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(Color.highlighterInkStrong)
                    if let about = profile?.about, !about.isEmpty {
                        Text(about)
                            .font(.caption)
                            .foregroundStyle(Color.highlighterInkMuted)
                            .lineLimit(2)
                    }
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkMuted)
            }
            .padding(12)
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .stroke(Color.highlighterRule, lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
        .task {
            profile = appStore.profileCache[pubkey]
            if profile == nil {
                await appStore.requestProfile(pubkeyHex: pubkey)
                profile = appStore.profileCache[pubkey]
            }
        }
    }
}

/// Fallback: any other kind. Show the kind, content snippet, author.
private struct GenericEntityCard: View {
    let event: NostrEntityEvent

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("KIND \(event.kind)")
                .font(.caption2.weight(.semibold))
                .tracking(0.8)
                .foregroundStyle(Color.highlighterInkMuted)
            if !event.content.isEmpty {
                Text(event.content)
                    .font(.callout)
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(4)
            }
            Text(String(event.pubkeyHex.prefix(12)) + "…")
                .font(.caption.monospaced())
                .foregroundStyle(Color.highlighterInkMuted)
        }
        .padding(12)
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.highlighterRule, lineWidth: 1)
        )
    }
}

// MARK: - Helpers

private func parseTags(_ json: String) -> [[String]] {
    guard let data = json.data(using: .utf8) else { return [] }
    return (try? JSONDecoder().decode([[String]].self, from: data)) ?? []
}

private func tagValue(_ tags: [[String]], _ name: String) -> String {
    for t in tags where t.first == name && t.count > 1 {
        return t[1]
    }
    return ""
}

private func relativeDate(_ secondsSinceEpoch: UInt64) -> String {
    let date = Date(timeIntervalSince1970: TimeInterval(secondsSinceEpoch))
    let formatter = RelativeDateTimeFormatter()
    formatter.unitsStyle = .abbreviated
    return formatter.localizedString(for: date, relativeTo: Date())
}
