import SwiftUI
// OWNER: Phase-2 Agent C (Accounts / multi-session). Replace whole file.
struct AccountsView: View {
    @EnvironmentObject private var model: KernelModel
    var body: some View {
        ChirpPlaceholder(systemImage: "person.2.fill", title: "Accounts",
            subtitle: "Agent C builds this.")
            .navigationTitle("Accounts")
    }
}
