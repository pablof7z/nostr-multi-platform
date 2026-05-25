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
        // User pages — need a `ProfileWire`. While the kernel is still
        // fetching kind:0 for the demo pubkey, `model.demoProfile` is nil
        // and the page shows a `ProgressView`.
        case "user-avatar":
            UserAvatarPage(profile: model.demoProfile)
        case "user-name":
            UserProfileNamePage(profile: model.demoProfile)
        case "user-nip05":
            UserNip05Page(profile: model.demoProfile)
        case "user-npub":
            UserNpubPage(profile: model.demoProfile)
        case "user-card":
            UserCardPage(profile: model.demoProfile)
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
        default:
            Text("Unknown component: \(component.id)")
                .foregroundStyle(.secondary)
        }
    }
}
