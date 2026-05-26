import SwiftUI

/// Detail column / pushed page: dispatches on `component.id` and shows the
/// right page view. The page views live in `UserComponentPages.swift` and
/// `ContentComponentPages.swift`.
struct ComponentDetailView: View {
    let component: RegistryComponent
    @Environment(GalleryModel.self) private var model

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                header
                Divider()
                pageBody
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(20)
        }
        .background(Color(.systemGroupedBackground))
        .nostrContentRenderer(NostrContentRenderer())
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(component.id)
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
            Text(component.label)
                .font(.title2.weight(.semibold))
            Text(component.description)
                .font(.callout)
                .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private var pageBody: some View {
        switch component.id {
        // Relay pages — static sample data; no kernel connection needed.
        case "relay-list":
            RelayListPage()
        // User pages — never block on relay data. `bestEffortProfile`
        // returns a placeholder `ProfileWire` (identicon + truncated npub)
        // before kind:0 arrives; the registry components degrade gracefully
        // on missing optional fields. SwiftUI re-renders automatically
        // when the real profile lands because `GalleryModel` is
        // `@Observable`.
        case "user-avatar":
            UserAvatarPage(profile: model.bestEffortProfile)
        case "user-name":
            UserProfileNamePage(profile: model.bestEffortProfile)
        case "user-nip05":
            UserNip05Page(profile: model.bestEffortProfile)
        case "user-npub":
            UserNpubPage(profile: model.bestEffortProfile)
        case "user-card":
            UserCardPage(profile: model.bestEffortProfile)
        // Content pages — work without relay data; the wire trees are
        // constructed in-line inside each page builder.
        case "content-core":
            ContentCorePage()
        case "content-view":
            ContentViewPage()
        case "content-mention-chip":
            ContentMentionChipPage()
        case "content-minimal":
            ContentMinimalPage()
        case "content-media-grid":
            ContentMediaGridPage()
        case "content-quote-card":
            ContentQuoteCardPage()
        // Embed pages — exercise the renderer-driven claim path
        // (ADR-0034 / M16). Each page builds a tree with a real bech32
        // URI; `EmbeddedEvent` fires the claim and the kernel resolves
        // through the OneshotApi.
        case "embed-article":
            ArticleEmbedPage()
        case "embed-profile":
            ProfileEmbedPage()
        case "embed-note":
            NoteEmbedPage()
        case "embed-highlight":
            HighlightEmbedPage()
        default:
            Text("Unknown component: \(component.id)")
                .foregroundStyle(.secondary)
        }
    }
}
