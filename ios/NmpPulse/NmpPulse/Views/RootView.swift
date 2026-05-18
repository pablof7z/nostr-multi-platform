import SwiftUI

/// Top-level navigation. T66a wires all five screens:
///
/// - No active account → `OnboardingView` (full-screen, no chrome).
/// - Active account → `TabView`: Timeline (→ NoteDetail, + Compose),
///   Accounts (switcher + relays), Diagnostics, More.
///
/// The gate is `model.hasActiveAccount`, read straight off the kernel
/// snapshot — there is no Swift-side session flag (D5/D8).
struct RootView: View {
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        Group {
            if model.hasActiveAccount {
                signedInTabs
            } else {
                OnboardingView()
            }
        }
        .task {
            model.start()
        }
    }

    private var signedInTabs: some View {
        TabView {
            NavigationStack {
                TimelineView()
            }
            .tabItem { Label("Timeline", systemImage: "house") }

            NavigationStack {
                AccountsView()
            }
            .tabItem { Label("Accounts", systemImage: "person.2") }

            NavigationStack {
                DiagnosticsView()
            }
            .tabItem { Label("Diagnostics", systemImage: "gauge") }

            NavigationStack {
                PendingFeaturesView()
            }
            .tabItem { Label("More", systemImage: "ellipsis.circle") }
        }
    }
}
