import SwiftUI

struct WalletView: View {
    @EnvironmentObject private var model: KernelModel
    @State private var showingConnectSheet = false

    var body: some View {
        List {
            if let status = model.walletStatus, status.isConnected {
                connectedSection(status: status)
                if status.isReady {
                    walletActionsSection(status: status)
                }
            } else {
                disconnectedSection
            }
            technologySection
        }
        .navigationTitle("Wallet")
        .navigationBarTitleDisplayMode(.large)
        .sheet(isPresented: $showingConnectSheet) {
            ConnectWalletSheet(isPresented: $showingConnectSheet)
                .environmentObject(model)
        }
    }

    // ── Disconnected state ──────────────────────────────────────────────────

    private var disconnectedSection: some View {
        Section {
            VStack(spacing: 16) {
                Image(systemName: "bolt.circle")
                    .font(.system(size: 52, weight: .light))
                    .foregroundStyle(.orange)
                    .symbolRenderingMode(.hierarchical)

                VStack(spacing: 8) {
                    Text("Connect a Wallet")
                        .font(.title2.weight(.semibold))
                    Text("Use any NWC-compatible wallet — Alby, Zeus, Mutiny, or self-hosted — to send and receive Lightning payments.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                }

                Button {
                    showingConnectSheet = true
                } label: {
                    Label("Connect Wallet", systemImage: "bolt.fill")
                }
                .buttonStyle(.borderedProminent)
                .tint(.orange)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 24)
        }
    }

    // ── Connected state ───────────────────────────────────────────────────────

    private func connectedSection(status: WalletStatusData) -> some View {
        Section {
            VStack(spacing: 16) {
                HStack {
                    HStack(spacing: 5) {
                        Circle()
                            .fill(statusColor(status.status))
                            .frame(width: 6, height: 6)
                        Text(status.status.capitalized)
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(statusColor(status.status))
                    }
                    Spacer()
                    Button(role: .destructive) {
                        model.walletDisconnect()
                    } label: {
                        Text("Disconnect")
                            .font(.caption)
                    }
                }

                Image(systemName: "bolt.circle.fill")
                    .font(.system(size: 48, weight: .light))
                    .foregroundStyle(.orange)
                    .symbolRenderingMode(.hierarchical)

                if let sats = status.balanceSats {
                    Text("\(sats.formatted()) sats")
                        .font(.largeTitle.weight(.bold))
                } else {
                    Text(status.status == "connecting" ? "Fetching balance…" : "— sats")
                        .font(.largeTitle.weight(.bold))
                        .foregroundStyle(.secondary)
                }

                Text(shortNpub(status.walletNpub))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 16)
        }
    }

    private func statusColor(_ status: String) -> Color {
        switch status {
        case "ready": return .green
        case "connecting": return .orange
        case "error": return .red
        default: return .secondary
        }
    }

    // ── Wallet actions ────────────────────────────────────────────────────────

    private func walletActionsSection(status: WalletStatusData) -> some View {
        Section("Actions") {
            HStack(spacing: 12) {
                Image(systemName: "arrow.up.circle.fill")
                    .foregroundStyle(.orange)
                Text("Send")
                    .font(.callout.weight(.semibold))
                Spacer()
                Text("Paste invoice to pay")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    // ── Technology ────────────────────────────────────────────────────────────

    private var technologySection: some View {
        Section("Powered By") {
            HStack(spacing: 12) {
                TechTile(label: "NWC", sublabel: "Nostr Wallet Connect", color: .orange)
                TechTile(label: "NIP-57", sublabel: "Zap protocol", color: .accentColor)
                TechTile(label: "Cashu", sublabel: "Ecash tokens", color: .brown)
            }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    private func shortNpub(_ npub: String) -> String {
        guard npub.count > 16 else { return npub }
        return "\(npub.prefix(10))…\(npub.suffix(6))"
    }
}

// ── Connect Wallet Sheet ───────────────────────────────────────────────────

private struct ConnectWalletSheet: View {
    @EnvironmentObject private var model: KernelModel
    @Binding var isPresented: Bool
    @State private var uri = ""

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextEditor(text: $uri)
                        .font(.body.monospaced())
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .frame(minHeight: 100)
                        .overlay(alignment: .topLeading) {
                            if uri.isEmpty {
                                Text("nostr+walletconnect://…")
                                    .font(.body.monospaced())
                                    .foregroundStyle(.secondary)
                                    .allowsHitTesting(false)
                                    .padding(.top, 8)
                                    .padding(.leading, 4)
                            }
                        }
                } header: {
                    Text("Paste your NWC connection string from Alby, Zeus, Mutiny, or any NIP-47 compatible wallet.")
                }

                Section {
                    Button {
                        let trimmed = uri.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !trimmed.isEmpty else { return }
                        model.walletConnect(uri: trimmed)
                        isPresented = false
                    } label: {
                        Label("Connect", systemImage: "bolt.fill")
                    }
                    .disabled(!schemeLooksValid(uri))
                    .buttonStyle(.borderedProminent)
                    .tint(.orange)
                }
            }
            .navigationTitle("Connect Wallet")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { isPresented = false }
                }
            }
        }
    }

    private func schemeLooksValid(_ s: String) -> Bool {
        let trimmed = s.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.lowercased().hasPrefix("nostr+walletconnect://")
    }
}

// ── Technology tile ────────────────────────────────────────────────────────

private struct TechTile: View {
    let label: String
    let sublabel: String
    let color: Color

    var body: some View {
        VStack(spacing: 4) {
            Text(label)
                .font(.headline)
                .foregroundStyle(color)
            Text(sublabel)
                .font(.caption)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 10))
    }
}
