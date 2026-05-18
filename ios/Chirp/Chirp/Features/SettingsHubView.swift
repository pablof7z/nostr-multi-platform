import SwiftUI
// OWNER: Phase-2 Agent C (Settings hub: links Accounts, Relays,
// Diagnostics, About). Replace whole file. Use NavigationLink to
// AccountsView() and DiagnosticsView().
struct SettingsHubView: View {
    @EnvironmentObject private var model: KernelModel
    var body: some View {
        ChirpPlaceholder(systemImage: "gearshape.fill", title: "Settings",
            subtitle: "Agent C builds this.")
            .navigationTitle("Settings")
    }
}
