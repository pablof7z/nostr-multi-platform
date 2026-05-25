import SwiftUI

/// Entry point for the NMP component gallery — a developer tool that browses
/// the registry's SwiftUI components with real Nostr data piped through the
/// NMP kernel.
///
/// Architectural rule (CRITICAL): all relay / network I/O happens inside the
/// kernel actor that `GalleryKernelHandle` wraps. There is zero
/// `URLSessionWebSocketTask` code in this app; profile data flows through
/// `nmp_app_claim_profile` and arrives in the kernel snapshot.
///
/// Screenshot mode: when launched with `--component <slug>` (or env var
/// `NMP_GALLERY_COMPONENT=<slug>`), the app skips `GalleryNavigation` and
/// renders just the component's detail page. Used by the screenshot
/// automation pipeline.
@main
struct NmpGalleryApp: App {
    @State private var model = GalleryModel()

    var body: some Scene {
        WindowGroup {
            rootView
                .environment(model)
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
        for section in REGISTRY_SECTIONS {
            if let match = section.components.first(where: { $0.id == slug }) {
                return match
            }
        }
        return nil
    }
}
