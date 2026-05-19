import Kingfisher
import SwiftUI

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
