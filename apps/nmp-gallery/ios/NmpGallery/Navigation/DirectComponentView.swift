import SwiftUI

/// Screenshot-only entry point: renders one component's detail page without
/// the surrounding navigation chrome. Selected by `NmpGalleryApp` when the
/// `--component <slug>` launch argument (or `NMP_GALLERY_COMPONENT` env var)
/// is set.
///
/// Renders the same `pageBody` switch as `ComponentDetailView` so the
/// screenshots match what users see in the live gallery — minus the
/// navigation toolbar / sidebar.
struct DirectComponentView: View {
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
        case "user-avatar":
            UserAvatarPage(pubkey: SHOWCASE_PUBKEY_HEX)
        case "user-name":
            UserProfileNamePage(profile: model.bestEffortProfile)
        case "user-nip05":
            UserNip05Page(profile: model.bestEffortProfile)
        case "user-npub":
            UserNpubPage(profile: model.bestEffortProfile)
        case "user-card":
            UserCardPage(profile: model.bestEffortProfile)
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
        case "relay-list":
            RelayListPage()
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
