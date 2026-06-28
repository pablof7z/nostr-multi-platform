import SwiftUI

/// Renderer protocol for one `EmbedKindProjection` variant (or a group of
/// unknown numeric kinds). Mirrors the TUI's `KindRenderer` trait — the body
/// is the SwiftUI view emitted for the projection.
///
/// Implementations are `@MainActor` because they read SwiftUI environment
/// state through the `NostrContentRenderer` injected into the view tree.
@MainActor
protocol KindRenderer {
    /// Render the projection into a SwiftUI view.
    ///
    /// `registry` is provided so recursive kind dispatch is possible (e.g. an
    /// article renderer that wants to nest a short-note preview through the
    /// same dispatch path).
    func body(
        projection: EmbedKindProjection,
        registry: NostrKindRegistry
    ) -> AnyView
}

/// Single source of truth for kind → SwiftUI renderer dispatch on iOS.
///
/// Built up via `set_*` setters or `register_unknown(kind:)`; consulted by
/// `EmbeddedEvent` (and by `NostrContentView` when it walks an `EventRef`
/// node). Mirrors the TUI's `NostrKindRegistry` shape so apps porting
/// renderers from the terminal can do so 1:1.
@MainActor
final class NostrKindRegistry: ObservableObject {
    private var shortNote: KindRenderer?
    private var article: KindRenderer?
    private var highlight: KindRenderer?
    private var profile: KindRenderer?
    private var unknownByKind: [UInt32: KindRenderer] = [:]
    private var fallback: KindRenderer

    init(fallback: KindRenderer = DefaultUnknownRenderer()) {
        self.fallback = fallback
    }

    /// Returns a registry pre-populated with the built-in defaults for every
    /// known projection variant. Replace any slot via `setArticle(...)` /
    /// `setShortNote(...)` to swap in a richer handler.
    static func makeDefault() -> NostrKindRegistry {
        let reg = NostrKindRegistry()
        reg.shortNote = DefaultShortNoteRenderer()
        reg.article = DefaultArticleRenderer()
        reg.highlight = DefaultHighlightRenderer()
        reg.profile = DefaultProfileRenderer()
        return reg
    }

    func setShortNote(_ renderer: KindRenderer) { shortNote = renderer }
    func setArticle(_ renderer: KindRenderer) { article = renderer }
    func setHighlight(_ renderer: KindRenderer) { highlight = renderer }
    func setProfile(_ renderer: KindRenderer) { profile = renderer }
    func registerUnknown(kind: UInt32, renderer: KindRenderer) {
        unknownByKind[kind] = renderer
    }

    /// Resolve the renderer responsible for a projection.
    func resolve(_ projection: EmbedKindProjection) -> KindRenderer {
        switch projection {
        case .shortNote:
            return shortNote ?? fallback
        case .article:
            return article ?? fallback
        case .highlight:
            return highlight ?? fallback
        case .profile:
            return profile ?? fallback
        case .unknown(let payload):
            return unknownByKind[payload.kind] ?? fallback
        }
    }
}

// MARK: - Default renderers

/// Default short-note renderer. Shows author + content text.
struct DefaultShortNoteRenderer: KindRenderer {
    init() {}
    func body(
        projection: EmbedKindProjection,
        registry: NostrKindRegistry
    ) -> AnyView {
        guard case .shortNote(let note) = projection else { return AnyView(EmptyView()) }
        let author = note.authorDisplayName ?? shortHex(note.authorPubkey)
        return AnyView(
            VStack(alignment: .leading, spacing: 4) {
                Text("note · \(author)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(note.content)
                    .font(.callout)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        )
    }
}

/// Default profile renderer. Shows display name + about line.
struct DefaultProfileRenderer: KindRenderer {
    init() {}
    func body(
        projection: EmbedKindProjection,
        registry: NostrKindRegistry
    ) -> AnyView {
        guard case .profile(let p) = projection else { return AnyView(EmptyView()) }
        let label = p.displayName ?? shortHex(p.pubkey)
        return AnyView(
            VStack(alignment: .leading, spacing: 4) {
                Text("profile")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(label).font(.headline)
                if let about = p.about, !about.isEmpty {
                    Text(about)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            }
        )
    }
}

/// Default article renderer. Shows title · author · summary. The richer
/// `ArticleEmbed` lives in `content-kind-30023/` and overrides this slot.
struct DefaultArticleRenderer: KindRenderer {
    init() {}
    func body(
        projection: EmbedKindProjection,
        registry: NostrKindRegistry
    ) -> AnyView {
        guard case .article(let a) = projection else { return AnyView(EmptyView()) }
        let author = a.authorDisplayName ?? shortHex(a.authorPubkey)
        let title = a.title ?? "article"
        let summary = a.summary ?? ""
        return AnyView(
            VStack(alignment: .leading, spacing: 4) {
                Text("\(title) · \(author)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(summary)
                    .font(.callout)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        )
    }
}

/// Default highlight renderer. Shows highlighted text + source author. The
/// richer `HighlightEmbed` lives in `content-kind-9802/`.
struct DefaultHighlightRenderer: KindRenderer {
    init() {}
    func body(
        projection: EmbedKindProjection,
        registry: NostrKindRegistry
    ) -> AnyView {
        guard case .highlight(let h) = projection else { return AnyView(EmptyView()) }
        let author = h.authorDisplayName ?? shortHex(h.authorPubkey)
        return AnyView(
            VStack(alignment: .leading, spacing: 4) {
                Text("highlight · \(author)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(h.highlightedText)
                    .font(.callout.italic())
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        )
    }
}

/// Fallback renderer for `EmbedKindProjection.unknown` — numeric kinds without
/// a registered handler.
struct DefaultUnknownRenderer: KindRenderer {
    init() {}
    func body(
        projection: EmbedKindProjection,
        registry: NostrKindRegistry
    ) -> AnyView {
        guard case .unknown(let u) = projection else { return AnyView(EmptyView()) }
        let author = u.authorDisplayName ?? shortHex(u.authorPubkey)
        return AnyView(
            VStack(alignment: .leading, spacing: 4) {
                Text("kind:\(u.kind) · \(author)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(u.content)
                    .font(.callout)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        )
    }
}

/// Truncate a hex pubkey/event-id for display in default renderers.
private func shortHex(_ value: String) -> String {
    guard value.count > 8 else { return value }
    return String(value.prefix(8))
}
