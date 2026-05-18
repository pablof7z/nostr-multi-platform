import SwiftUI

struct SettingsView: View {
    @Environment(HighlighterStore.self) private var store
    @Environment(\.dismiss) private var dismiss

    @State private var showLogoutConfirm = false

    var body: some View {
        NavigationStack {
            List {
                accountSection
                connectionsSection
                keysSection
                aboutSection
                logOutSection
            }
            .listStyle(.insetGrouped)
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                        .fontWeight(.semibold)
                }
            }
            .confirmationDialog(
                "Log out of Highlighter?",
                isPresented: $showLogoutConfirm,
                titleVisibility: .visible
            ) {
                Button("Log Out", role: .destructive) {
                    store.logout()
                    dismiss()
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("You'll need your signer to sign back in.")
            }
        }
    }

    // MARK: - Sections

    @ViewBuilder
    private var accountSection: some View {
        if let user = store.currentUser {
            Section {
                HStack(spacing: 16) {
                    AuthorAvatar(
                        pubkey: user.pubkey,
                        pictureURL: store.currentUserProfile?.picture ?? "",
                        displayInitial: displayInitial,
                        size: 68
                    )
                    VStack(alignment: .leading, spacing: 4) {
                        Text(profileDisplayName)
                            .font(.title3.weight(.semibold))
                        Text(shortenedNpub(user.npub))
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                            .monospaced()
                            .lineLimit(1)
                    }
                    Spacer()
                }
                .padding(.vertical, 6)
            }
        }
    }

    private var connectionsSection: some View {
        Section {
            NavigationLink {
                NetworkSettingsView()
            } label: {
                Label("Network", systemImage: "network")
            }
            NavigationLink {
                MediaSettingsView()
            } label: {
                Label("Media", systemImage: "photo.on.rectangle.angled")
            }
        }
    }

    private var keysSection: some View {
        Section {
            NavigationLink {
                KeysView()
            } label: {
                Label("Secret Key", systemImage: "key.fill")
            }
        } header: {
            Text("Keys")
        } footer: {
            Text("Your nsec is the master key to your Nostr identity. Never share it.")
        }
    }

    private var aboutSection: some View {
        Section("About") {
            LabeledContent("Version", value: appVersionString)
        }
    }

    private var logOutSection: some View {
        Section {
            Button(role: .destructive) {
                showLogoutConfirm = true
            } label: {
                HStack {
                    Spacer()
                    Text("Log Out")
                        .fontWeight(.semibold)
                    Spacer()
                }
            }
        }
    }

    // MARK: - Helpers

    private var profileDisplayName: String {
        if let profile = store.currentUserProfile {
            let name = profile.displayName.isEmpty ? profile.name : profile.displayName
            if !name.isEmpty { return name }
        }
        return "Nostr Account"
    }

    private var displayInitial: String {
        if let profile = store.currentUserProfile {
            let name = profile.displayName.isEmpty ? profile.name : profile.displayName
            return name
        }
        return ""
    }

    private func shortenedNpub(_ npub: String) -> String {
        guard npub.count > 20 else { return npub }
        return "\(npub.prefix(10))…\(npub.suffix(8))"
    }

    private var appVersionString: String {
        let info = Bundle.main.infoDictionary
        let version = info?["CFBundleShortVersionString"] as? String ?? "—"
        let build = info?["CFBundleVersion"] as? String ?? "—"
        return "\(version) (\(build))"
    }
}
