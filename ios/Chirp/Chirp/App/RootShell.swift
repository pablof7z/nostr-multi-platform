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
        // D7 actor-death banner. Layered AFTER the toast so it renders on
        // top — and unconditionally: a panic is terminal, so the banner
        // cannot be dismissed (the user must relaunch). The flag flips on
        // either the push-side panic frame (KernelModel.init's onPanic
        // closure) or the pull-side liveness probe (ChirpApp's scenePhase
        // active arm), so a kernel that dies while the app is backgrounded
        // is also caught.
        .overlay(alignment: .top) { kernelDeadBanner }
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
        if let msg = model.lastSuccessToast {
            Text(msg)
                .font(ChirpFont.callout)
                .foregroundStyle(ChirpColor.positive)
                .padding(.horizontal, ChirpSpace.l).padding(.vertical, ChirpSpace.m)
                .background(.regularMaterial, in: Capsule())
                .padding(.top, 8)
                .onTapGesture { model.clearSuccessToast() }
                .task {
                    try? await Task.sleep(for: .seconds(3))
                    model.clearSuccessToast()
                }
        } else if let msg = model.localizedErrorToast {
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

    // D7 actor-death banner — see `var body` for the overlay layering.
    //
    // The Rust actor thread that owns the kernel loop has died (panic, or a
    // post-Shutdown state observed via the `nmp_app_is_alive` probe on
    // foreground resume — ADR-0028). Every subsequent FFI command is a
    // silent no-op; the timeline is frozen and "Send" taps do nothing. The
    // only safe recovery is a process restart — restarting the actor
    // in-process is unsafe because the kernel's event store, MLS DB, and
    // NIP-77 watermarks are in an unknown state after a panic.
    //
    // The banner is full-width danger, non-dismissible (no tap-to-clear
    // gesture — the flag is a stuck-at-true latch in `KernelModel`), and
    // offers a single "Relaunch" button that calls `exit(0)`. `exit(0)` on
    // iOS is the canonical "force the user back to the home screen" — the OS
    // will re-spawn the app on the next launch with a fresh `NmpApp`.
    @ViewBuilder
    private var kernelDeadBanner: some View {
        if model.kernelIsDead {
            VStack(alignment: .leading, spacing: ChirpSpace.s) {
                Text("Background service stopped")
                    .font(ChirpFont.headline)
                    .foregroundStyle(ChirpColor.emphasisForeground)
                Text("Please relaunch the app to recover.")
                    .font(ChirpFont.callout)
                    .foregroundStyle(ChirpColor.emphasisForeground.opacity(0.92))
                Button {
                    // exit(0) is the iOS-canonical "force-quit": the OS
                    // tears the process down and the user re-launches
                    // from the home screen with a fresh kernel. A graceful
                    // `stop()` is meaningless here — the actor is gone.
                    exit(0)
                } label: {
                    Text("Relaunch")
                        .font(ChirpFont.callout.weight(.semibold))
                        .padding(.horizontal, ChirpSpace.l)
                        .padding(.vertical, ChirpSpace.s)
                        .background(ChirpColor.emphasisForeground, in: Capsule())
                        .foregroundStyle(ChirpColor.danger)
                }
                .accessibilityIdentifier("kernel-dead-relaunch-button")
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, ChirpSpace.l)
            .padding(.vertical, ChirpSpace.m)
            .background(ChirpColor.errorBannerBackground)
            .accessibilityIdentifier("kernel-dead-banner")
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
