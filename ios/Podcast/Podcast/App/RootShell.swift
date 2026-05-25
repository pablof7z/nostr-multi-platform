import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// NAVIGATION CONTRACT — M0.B skeleton.
//
// Single "Pod0" placeholder tab. Feature tabs are added in later milestones.
// One NavigationStack per tab. Typed `PodcastRoute` destinations are
// resolved centrally here so Profile/Thread work identically from any tab.
// ─────────────────────────────────────────────────────────────────────────

/// Typed navigation routes. Push with `router.path.append(.profile(pk))`.
enum PodcastRoute: Hashable {
    case profile(pubkey: String)
    case thread(eventID: String)
}

/// Per-tab navigation path holder injected into the environment.
@MainActor
final class PodcastRouter: ObservableObject {
    @Published var path = NavigationPath()
    func push(_ r: PodcastRoute) { path.append(r) }
    func popToRoot() { path = NavigationPath() }
}

enum PodcastTab: Hashable { case home }

struct RootShell: View {
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        mainTabs
            .podcastScreenBackground()
            .overlay(alignment: .top) { toast }
            // D7 actor-death banner. Layered after the toast so it renders
            // on top — unconditionally: a panic is terminal and the banner
            // cannot be dismissed until relaunch.
            .overlay(alignment: .top) { kernelDeadBanner }
    }

    private var mainTabs: some View {
        TabView {
            tabStack { Pod0PlaceholderView() }
                .tabItem { Label("Pod0", systemImage: "play.circle.fill") }
                .tag(PodcastTab.home)
        }
        .toolbarBackground(.visible, for: .tabBar)
        .toolbarBackground(.regularMaterial, for: .tabBar)
    }

    @ViewBuilder
    private func tabStack<Root: View>(@ViewBuilder _ root: () -> Root) -> some View {
        PodcastTabStack(root: root())
    }

    @ViewBuilder
    private var toast: some View {
        if let msg = model.lastErrorToast {
            Text(msg)
                .font(PodcastFont.callout)
                .foregroundStyle(.primary)
                .padding(.horizontal, PodcastSpace.l).padding(.vertical, PodcastSpace.m)
                .background(.regularMaterial, in: Capsule())
                .padding(.top, 8)
                .onTapGesture { model.clearErrorToast() }
                .task {
                    try? await Task.sleep(for: .seconds(4))
                    model.clearErrorToast()
                }
        }
    }

    // D7 actor-death banner.
    //
    // The Rust actor thread that owns the kernel loop has died (panic, or a
    // post-Shutdown state observed via the `nmp_app_is_alive` probe on
    // foreground resume — ADR-0028). Every subsequent FFI command is a
    // silent no-op; the only safe recovery is a process restart.
    @ViewBuilder
    private var kernelDeadBanner: some View {
        if model.kernelIsDead {
            VStack(alignment: .leading, spacing: PodcastSpace.s) {
                Text("Background service stopped")
                    .font(PodcastFont.headline)
                    .foregroundStyle(PodcastColor.emphasisForeground)
                Text("Please relaunch the app to recover.")
                    .font(PodcastFont.callout)
                    .foregroundStyle(PodcastColor.emphasisForeground.opacity(0.92))
                Button {
                    exit(0)
                } label: {
                    Text("Relaunch")
                        .font(PodcastFont.callout.weight(.semibold))
                        .padding(.horizontal, PodcastSpace.l)
                        .padding(.vertical, PodcastSpace.s)
                        .background(PodcastColor.emphasisForeground, in: Capsule())
                        .foregroundStyle(PodcastColor.danger)
                }
                .accessibilityIdentifier("kernel-dead-relaunch-button")
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, PodcastSpace.l)
            .padding(.vertical, PodcastSpace.m)
            .background(PodcastColor.errorBannerBackground)
            .accessibilityIdentifier("kernel-dead-banner")
        }
    }
}

private struct PodcastTabStack<Root: View>: View {
    let root: Root
    @StateObject private var router = PodcastRouter()
    var body: some View {
        NavigationStack(path: $router.path) {
            root
                .navigationDestination(for: PodcastRoute.self) { route in
                    switch route {
                    case .profile(let pk): Text("Profile: \(pk)")
                    case .thread(let id): Text("Thread: \(id)")
                    }
                }
        }
        .environmentObject(router)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Pod0PlaceholderView — replaced in later milestones with real podcast UI.
// ─────────────────────────────────────────────────────────────────────────

struct Pod0PlaceholderView: View {
    var body: some View {
        PodcastPlaceholder(
            systemImage: "play.circle",
            title: "Pod0",
            subtitle: "Podcast player coming soon.")
        .navigationTitle("Pod0")
    }
}
