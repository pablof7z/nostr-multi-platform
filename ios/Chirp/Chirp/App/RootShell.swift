import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// NAVIGATION CONTRACT — updated 2026-05-21 (Groups/Chats UX redesign).
//
// User-authorized override of previous "FROZEN" comment:
//   • `search` tab removed (Search deferred to toolbar button on HomeFeed)
//   • `chats` tab added (slot 2) — DM inbox via ChatsView
//   • `groups` tab updated — unified flat list via GroupsView
//   • Tab order: home · chats · groups · wallet · settings
//
// Every feature screen is its own file under Features/. Agents replace the
// BODY of their assigned feature file; they do not touch ChirpRoute or
// each other's files.
//
// Navigation: one NavigationStack per tab. Any screen pushes a typed
// `ChirpRoute`; destinations are resolved centrally here so Profile/Thread
// work identically from any tab without cross-file coupling.
// ─────────────────────────────────────────────────────────────────────────

/// Typed navigation routes. Push with `router.path.append(.profile(pk))`.
enum ChirpRoute: Hashable {
    case profile(pubkey: String)
    case thread(eventID: String)
}

/// Per-tab navigation path holder injected into the environment.
@MainActor
final class ChirpRouter: ObservableObject {
    @Published var path = NavigationPath()
    func push(_ r: ChirpRoute) { path.append(r) }
    func popToRoot() { path = NavigationPath() }
}

enum ChirpTab: Hashable { case home, chats, groups, wallet, settings }

struct RootShell: View {
    @EnvironmentObject private var model: KernelModel
    @State private var tab: ChirpTab = .home

    var body: some View {
        Group {
            if model.hasActiveAccount {
                mainTabs
            } else {
                OnboardingView()
            }
        }
        .chirpScreenBackground()
        .overlay(alignment: .top) { toast }
    }

    private var mainTabs: some View {
        TabView(selection: $tab) {
            tabStack { HomeFeedView() }
                .tabItem { Label("Home", systemImage: "house.fill") }
                .tag(ChirpTab.home)

            tabStack { ChatsView() }
                .tabItem { Label("Chats", systemImage: "bubble.left.and.bubble.right.fill") }
                .tag(ChirpTab.chats)

            tabStack { GroupsView() }
                .tabItem { Label("Groups", systemImage: "person.3.fill") }
                .tag(ChirpTab.groups)

            tabStack { WalletView() }
                .tabItem { Label("Wallet", systemImage: "bolt.fill") }
                .tag(ChirpTab.wallet)

            tabStack { SettingsHubView() }
                .tabItem { Label("Settings", systemImage: "gearshape.fill") }
                .tag(ChirpTab.settings)
        }
        .toolbarBackground(.visible, for: .tabBar)
        .toolbarBackground(.regularMaterial, for: .tabBar)
    }

    /// Wraps a tab root in its own NavigationStack + the shared route
    /// destination map + a per-tab router in the environment.
    @ViewBuilder
    private func tabStack<Root: View>(@ViewBuilder _ root: () -> Root) -> some View {
        TabStack(root: root())
    }

    @ViewBuilder
    private var toast: some View {
        if let msg = model.lastErrorToast {
            Text(msg)
                .font(ChirpFont.callout)
                .foregroundStyle(.primary)
                .padding(.horizontal, ChirpSpace.l).padding(.vertical, ChirpSpace.m)
                .background(.regularMaterial, in: Capsule())
                .padding(.top, 8)
                .onTapGesture { model.clearErrorToast() }
                .task {
                    try? await Task.sleep(for: .seconds(4))
                    model.clearErrorToast()
                }
        }
    }
}

private struct TabStack<Root: View>: View {
    let root: Root
    @StateObject private var router = ChirpRouter()
    var body: some View {
        NavigationStack(path: $router.path) {
            root
                .navigationDestination(for: ChirpRoute.self) { route in
                    switch route {
                    case .profile(let pk): ProfileView(pubkey: pk)
                    case .thread(let id): ThreadScreen(eventID: id)
                    }
                }
        }
        .environmentObject(router)
    }
}
