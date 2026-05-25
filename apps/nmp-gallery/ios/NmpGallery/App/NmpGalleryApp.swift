import SwiftUI

/// Entry point for the NMP component gallery — a developer tool that browses
/// the registry's SwiftUI components with real Nostr data piped through the
/// NMP kernel.
///
/// Architectural rule (CRITICAL): all relay / network I/O happens inside the
/// kernel actor that `GalleryKernelHandle` wraps. There is zero
/// `URLSessionWebSocketTask` code in this app; profile data flows through
/// `nmp_app_claim_profile` and arrives in the kernel snapshot.
@main
struct NmpGalleryApp: App {
    @State private var model = GalleryModel()

    var body: some Scene {
        WindowGroup {
            GalleryNavigation()
                .environment(model)
                .task {
                    model.start()
                }
        }
    }
}
