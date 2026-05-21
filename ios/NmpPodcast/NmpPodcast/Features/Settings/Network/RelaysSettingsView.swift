import SwiftUI

/// "Read Relays" settings screen. Lists the user's NIP-65 relays with live
/// status, role chips, and per-row navigation into `RelayDetailView`. The
/// toolbar `+` opens `AddRelaySheet`. State (relays + statuses) flows from
/// the kernel snapshot via `KernelModel`; this view never persists locally —
/// the Rust kernel is the source of truth.
struct RelaysSettingsView: View {
    @EnvironmentObject private var kernelModel: KernelModel
    @State private var showAddSheet = false
    @State private var pendingRemoveURL: String?

    var body: some View {
        List {
            headerSection
            if kernelModel.relays.isEmpty {
                emptySection
            } else {
                relaysSection
            }
            footerSection
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Read Relays")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    showAddSheet = true
                } label: {
                    Image(systemName: "plus")
                }
            }
        }
        .sheet(isPresented: $showAddSheet) {
            AddRelaySheet { url, read, write in
                kernelModel.addRelay(url: url, read: read, write: write)
            }
        }
        .confirmationDialog(
            "Remove this relay?",
            isPresented: Binding(
                get: { pendingRemoveURL != nil },
                set: { if !$0 { pendingRemoveURL = nil } }
            ),
            titleVisibility: .visible,
            presenting: pendingRemoveURL
        ) { url in
            Button("Remove", role: .destructive) {
                kernelModel.removeRelay(url: url)
            }
            Button("Cancel", role: .cancel) {}
        } message: { _ in
            Text("Podcastr will stop sending and receiving events through this relay.")
        }
    }

    // MARK: - Sections

    private var headerSection: some View {
        Section {
            HStack(spacing: 8) {
                Circle()
                    .fill(aggregateDotColor)
                    .frame(width: 10, height: 10)
                Text(aggregateLabel)
                    .font(.headline)
            }
            .padding(.vertical, 4)
        }
    }

    private var emptySection: some View {
        Section {
            ContentUnavailableView(
                "No relays",
                systemImage: "antenna.radiowaves.left.and.right",
                description: Text("Tap + to add a relay.")
            )
        }
    }

    private var relaysSection: some View {
        Section {
            ForEach(kernelModel.relays) { relay in
                NavigationLink {
                    RelayDetailView(url: relay.url)
                } label: {
                    RelayRowView(
                        relay: relay,
                        status: kernelModel.status(for: relay.url)
                    )
                }
                .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                    Button(role: .destructive) {
                        pendingRemoveURL = relay.url
                    } label: {
                        Label("Remove", systemImage: "trash")
                    }
                }
            }
        } header: {
            Text("Relays")
        } footer: {
            Text("Your Read and Write relays are published as a kind:10002 event. Other nostr users can see where you read and publish.")
        }
    }

    private var footerSection: some View {
        Section {
            EmptyView()
        } footer: {
            Text("Tap a relay to see traffic, change its roles, or remove it.")
        }
    }

    // MARK: - Derived

    private var connectedCount: Int {
        kernelModel.relayStatuses.filter { $0.isConnected }.count
    }

    private var aggregateLabel: String {
        let total = kernelModel.relays.count
        if total == 0 { return "No relays" }
        let online = connectedCount
        if online == 0 { return "Offline" }
        if online == total { return "Online — \(online) of \(total)" }
        return "\(online) of \(total) online"
    }

    private var aggregateDotColor: Color {
        if kernelModel.relays.isEmpty { return .gray }
        let online = connectedCount
        if online == 0 { return .red }
        if online == kernelModel.relays.count { return .green }
        return .yellow
    }
}
