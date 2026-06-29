import SwiftUI

/// Entry point for the NMP component gallery — a developer tool that browses
/// the registry's SwiftUI components with real Nostr data piped through the
/// NMP kernel.
///
/// Architectural rule (CRITICAL): all relay / network I/O happens inside the
/// kernel actor that `GalleryKernelHandle` wraps. There is zero
/// `URLSessionWebSocketTask` code in this app; profile data flows through
/// the unified ref-resolution seam (ADR-0063 #1671) and arrives in the kernel snapshot
/// via the `refs.profile` projection.
///
/// Screenshot mode: when launched with `--component <slug>` (or env var
/// `NMP_GALLERY_COMPONENT=<slug>`), the app skips `GalleryNavigation` and
/// renders just the component's detail page. Used by the screenshot
/// automation pipeline.
@main
struct NmpGalleryApp: App {
    @State private var model = GalleryModel()

    /// Kind-dispatch embed registry. Built once at app start with the
    /// gallery's richer per-kind components (ArticleEmbed, HighlightEmbed)
    /// installed on top of the defaults. Injected into the SwiftUI
    /// environment so every `NostrContentView` / `EmbeddedEvent` sees the
    /// same renderer table.
    @State private var kindRegistry: NostrKindRegistry = {
        let reg = NostrKindRegistry.makeDefault()
        reg.setArticle(ArticleEmbed())
        reg.setHighlight(HighlightEmbed())
        return reg
    }()

    var body: some Scene {
        WindowGroup {
            rootView
                .environment(model)
                .nmpComponentHost(
                    profileHost: model,
                    embedSource: model.embedHost,
                    eventRefResolver: model.embedEventRefResolver,
                    kindRegistry: kindRegistry
                )
                .task {
                    model.start()
                }
        }
    }

    @ViewBuilder
    private var rootView: some View {
        if let slug = Self.screenshotSlug,
           let component = Self.component(for: slug) {
            DirectComponentView(component: component)
        } else {
            GalleryNavigation()
        }
    }

    /// Pulls the requested component slug from the launch argument
    /// (`--component <slug>`) or the `NMP_GALLERY_COMPONENT` env var.
    static var screenshotSlug: String? {
        let args = CommandLine.arguments
        if let idx = args.firstIndex(of: "--component"), idx + 1 < args.count {
            return args[idx + 1]
        }
        if let env = ProcessInfo.processInfo.environment["NMP_GALLERY_COMPONENT"],
           !env.isEmpty {
            return env
        }
        return nil
    }

    /// Find the `RegistryComponent` row matching the given slug.
    static func component(for slug: String) -> RegistryComponent? {
        for section in GALLERY_SECTIONS {
            if let match = section.components.first(where: { $0.id == slug }) {
                return match
            }
        }
        return nil
    }
}
