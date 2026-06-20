import SwiftUI

@main
struct ChirpApp: App {
    @StateObject private var model = KernelModel()

    @State private var kindRegistry: NostrKindRegistry = {
        let reg = NostrKindRegistry.makeDefault()
        reg.setArticle(ArticleEmbed())
        reg.setHighlight(HighlightEmbed())
        return reg
    }()

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
                .environment(\.nostrProfileHost, model)
                .embedEnvelopeSource(
                    model.embedHost,
                    claimSink: model,
                    registry: kindRegistry
                )
                .tint(ChirpColor.accent)
                .task {
                    // Skip kernel boot when the app is launched as an XCTest
                    // host. Starting the kernel here saturates the main thread
                    // with the 4Hz snapshot→@MainActor apply storm, which
                    // starves the XCTest runner during its "preparing to run
                    // tests" phase and trips the runner-prepare timeout — no
                    // ChirpTests assertion ever executes. Unit tests construct
                    // their own `KernelModel()` and drive it directly, so the
                    // host runtime is unnecessary under test.
                    if ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] == nil {
                        model.start()
                    }
                }
                .onOpenURL { url in
                    guard url.scheme?.lowercased() == "chirp" else { return }
                    if let comps = URLComponents(url: url, resolvingAgainstBaseURL: false),
                       let bunkerUri = comps.queryItems?.first(where: { $0.name == "bunker_uri" || $0.name == "uri" })?.value,
                       bunkerUri.hasPrefix("bunker://") {
                        model.addSigner(bunkerUri: bunkerUri, makeActive: true)
                    }
                }
        }
        .onChange(of: scenePhase) { _, newPhase in
            // D7: Swift reports the fact; the kernel decides what each
            // phase MEANS (reconcile NIP-77 watermarks on Bg→Fg, throttle
            // retries on Fg→Bg, etc.). No policy lives here.
            switch newPhase {
            case .active:
                // ADR-0028: pull-side actor-liveness probe. If the app was
                // backgrounded across an actor panic, the push-side panic
                // frame may have arrived and the Swift listener thread may
                // have already exited (the update channel closed) before
                // the host had a chance to react. Probing here on every
                // foreground transition catches the missed signal and
                // surfaces the red banner so the user sees a fatal-error
                // state instead of a frozen UI. Probe BEFORE
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
