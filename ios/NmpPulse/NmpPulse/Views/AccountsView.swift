import SwiftUI

/// Screen 5 — multi-session switcher + relay editor.
///
/// - Accounts: tap a row → `nmp_app_switch_active` (synchronous active
///   switch; the kernel re-binds the signer + retargets the timeline before
///   the next snapshot). "+ Add" re-presents Onboarding.
/// - Relays: editable projection backed by `nmp_app_add_relay` /
///   `nmp_app_remove_relay`.
///
/// All rows are a pure mirror of the kernel snapshot (D5/D8). No Swift-side
/// account or relay state.
struct AccountsView: View {
    @EnvironmentObject private var model: KernelModel

    @State private var showAddAccount = false
    @State private var showAddRelay = false
    @State private var relayURL = ""
    @State private var relayRole = "both"

    var body: some View {
        List {
            Section("Accounts") {
                if model.accounts.isEmpty {
                    Text("No accounts — add one")
                        .foregroundStyle(.secondary)
                }
                ForEach(model.accounts) { account in
                    Button {
                        model.switchActive(account.id)
                    } label: {
                        HStack {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(account.displayName)
                                    .font(.subheadline).bold()
                                    .foregroundStyle(.primary)
                                Text(account.npub)
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                                    .truncationMode(.middle)
                            }
                            Spacer()
                            if account.isActive {
                                Image(systemName: "checkmark.circle.fill")
                                    .foregroundStyle(.tint)
                            }
                        }
                    }
                    .swipeActions {
                        Button(role: .destructive) {
                            model.removeAccount(account.id)
                        } label: {
                            Label("Remove", systemImage: "trash")
                        }
                    }
                }
                Button {
                    showAddAccount = true
                } label: {
                    Label("Add account", systemImage: "plus")
                }
            }

            Section {
                ForEach(model.relayEditRows) { relay in
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(relay.url)
                                .font(.subheadline)
                                .lineLimit(1)
                                .truncationMode(.middle)
                            Text(relay.role)
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                    }
                    .swipeActions {
                        Button(role: .destructive) {
                            model.removeRelay(url: relay.url)
                        } label: {
                            Label("Remove", systemImage: "trash")
                        }
                    }
                }
                Button {
                    showAddRelay = true
                } label: {
                    Label("Add relay", systemImage: "plus")
                }
            } header: {
                Text("Relays")
            } footer: {
                Text("Publishing resolves write-relays automatically from "
                     + "your kind:10002 (NIP-65 / D3) — no relay picker on Compose.")
                    .font(.caption2)
            }
        }
        .navigationTitle("Accounts")
        .sheet(isPresented: $showAddAccount) {
            NavigationStack {
                OnboardingView()
                    .navigationTitle("Add account")
                    .navigationBarTitleDisplayMode(.inline)
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) {
                            Button("Done") { showAddAccount = false }
                        }
                    }
            }
        }
        .onChange(of: model.accounts.count) { old, new in
            // Best-effort: dismiss the add sheet once a new account lands.
            if showAddAccount, new > old { showAddAccount = false }
        }
        .alert("Add relay", isPresented: $showAddRelay) {
            TextField("wss://relay.example", text: $relayURL)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
            Button("read") { addRelay("read") }
            Button("write") { addRelay("write") }
            Button("both") { addRelay("both") }
            Button("Cancel", role: .cancel) { relayURL = "" }
        } message: {
            Text("Enter the relay URL, then pick a role.")
        }
    }

    private func addRelay(_ role: String) {
        let url = relayURL.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !url.isEmpty else { return }
        model.addRelay(url: url, role: role)
        relayURL = ""
    }
}
