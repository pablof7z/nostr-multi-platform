import SwiftUI

struct DataStorageSettingsView: View {
    @Environment(AppStateStore.self) private var store
    @State private var storageSummary: String?
    @State private var showClearConfirmation = false

    var body: some View {
        List {
            dataSection
            storageSection
            destructiveSection
        }
        .settingsListStyle()
        .navigationTitle("Data & Storage")
        .navigationBarTitleDisplayMode(.inline)
        .task { await refreshStorageSummary() }
        .alert("Clear All Data?", isPresented: $showClearConfirmation) {
            Button("Cancel", role: .cancel) {}
            Button("Clear All Data", role: .destructive) {
                store.clearAllData()
                Haptics.success()
            }
        } message: {
            Text("This will permanently delete every subscription, episode, note, friend, memory, and agent activity entry. API credentials and your Nostr identity are preserved.")
        }
    }

    private var dataSection: some View {
        Section("Data") {
            NavigationLink {
                DataExportView()
            } label: {
                SettingsRow(
                    icon: "square.and.arrow.up",
                    tint: .teal,
                    title: "Export Data",
                    value: dataRecordCount > 0 ? "\(dataRecordCount) records" : nil
                )
            }
        }
    }

    private var storageSection: some View {
        Section("Storage") {
            NavigationLink {
                StorageSettingsView()
            } label: {
                SettingsRow(
                    icon: "internaldrive.fill",
                    tint: .gray,
                    title: "Downloads & Disk",
                    value: storageSummary
                )
            }
        }
    }

    private var destructiveSection: some View {
        Section {
            Button("Clear All Data", role: .destructive) {
                showClearConfirmation = true
            }
        } footer: {
            Text("Permanently deletes every subscription, episode, note, friend, memory, and agent activity entry. API credentials and your Nostr identity are preserved.")
        }
    }

    private var dataRecordCount: Int {
        store.state.subscriptions.count
            + store.state.episodes.count
            + store.activeNotes.count
            + store.activeMemories.count
            + store.state.friends.count
            + store.activeAgentActivityCount
    }

    private func refreshStorageSummary() async {
        let snap = await StorageSettingsView.compute(store: store)
        await MainActor.run {
            storageSummary = snap.totalBytes > 0 ? SettingsView.formatSize(snap.totalBytes) : nil
        }
    }
}
