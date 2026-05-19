import SwiftUI

@main
struct ChirpApp: App {
    @StateObject private var model = KernelModel()

    // T118 / G3 — iOS scenePhase observer. SwiftUI publishes
    // `.active` / `.inactive` / `.background` transitions; we route the
    // two terminal phases to the kernel and silently drop `.inactive` (the
    // app-switcher interstitial — the kernel's transition reducer would
    // debounce it anyway, but suppressing it at the call site avoids a
    // pointless FFI hop on every app-switch animation tick).
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            RootShell()
                .environmentObject(model)
                .tint(ChirpColor.accent)
                .preferredColorScheme(.dark)
                .task { model.start() }
                .onOpenURL { url in
                    guard url.scheme?.lowercased() == "chirp" else { return }
                    if let comps = URLComponents(url: url, resolvingAgainstBaseURL: false),
                       let bunkerUri = comps.queryItems?.first(where: { $0.name == "bunker_uri" || $0.name == "uri" })?.value,
                       bunkerUri.hasPrefix("bunker://") {
                        model.signInBunker(bunkerUri)
                    }
                }
        }
        .onChange(of: scenePhase) { _, newPhase in
            // D7: Swift reports the fact; the kernel decides what each
            // phase MEANS (reconcile NIP-77 watermarks on Bg→Fg, throttle
            // retries on Fg→Bg, etc.). No policy lives here.
            switch newPhase {
            case .active:
                model.lifecycleForeground()
            case .background:
                model.lifecycleBackground()
            case .inactive:
                break // transient — kernel never hears about it.
            @unknown default:
                break
            }
        }
    }
}
