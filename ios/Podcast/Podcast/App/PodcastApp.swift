import SwiftUI

@main
struct PodcastApp: App {
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
                .tint(PodcastColor.accent)
                .task { model.start() }
        }
        .onChange(of: scenePhase) { _, newPhase in
            // D7: Swift reports the fact; the kernel decides what each
            // phase MEANS. No policy lives here.
            switch newPhase {
            case .active:
                // ADR-0028: pull-side actor-liveness probe. Probe BEFORE
                // `lifecycleForeground` so a dead kernel does not also
                // get hit with a doomed lifecycle command.
                model.checkAlive()
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
