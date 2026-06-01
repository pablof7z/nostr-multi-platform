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
        .scrollContentBackground(.hidden)
        .chirpScreenBackground()
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
                    .foregroundStyle(ChirpColor.zap)
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
                .tint(ChirpColor.zap)
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
                    .foregroundStyle(ChirpColor.zap)
                    .symbolRenderingMode(.hierarchical)

                if let sats = status.balanceSats {
                    Text("\(sats.formatted(.number)) sats")
                        .font(.largeTitle.weight(.bold))
                } else {
                    Text(status.status == "connecting" ? "Fetching balance…" : "— sats")
                        .font(.largeTitle.weight(.bold))
                        .foregroundStyle(.secondary)
                }

                Text(status.walletPubkeyHex.shortHex)
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
        case "ready": return ChirpColor.success
        case "connecting": return ChirpColor.zap
        case "error": return ChirpColor.danger
        default: return ChirpColor.textSecondary
        }
    }

    // ── Wallet actions ────────────────────────────────────────────────────────

    private func walletActionsSection(status: WalletStatusData) -> some View {
        Section("Actions") {
            HStack(spacing: 12) {
                Image(systemName: "arrow.up.circle.fill")
                    .foregroundStyle(ChirpColor.zap)
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
                TechTile(label: "NWC", sublabel: "Nostr Wallet Connect", color: ChirpColor.zap)
                TechTile(label: "NIP-57", sublabel: "Zap protocol", color: ChirpColor.accent)
                TechTile(label: "Cashu", sublabel: "Ecash tokens", color: ChirpColor.textSecondary)
            }
        }
    }

    // V-23 thin-shell: `shortNpub` formerly lived here. The kernel now
    // projects `wallet_npub_short` (see `WalletStatus` in
    // `crates/nmp-core/src/actor/commands/wallet.rs`), and the connected
    // section binds `status.walletNpubShort` verbatim. No Swift-side display
    // formatting remains in this view.
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
                    // V-100: URI scheme validation moved to Rust (WalletConnectModule::start).
                    // The kernel rejects invalid URIs and surfaces the reason as a toast.
                    .disabled(uri.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .buttonStyle(.borderedProminent)
                    .tint(ChirpColor.zap)
                }
            }
            .scrollContentBackground(.hidden)
            .chirpScreenBackground()
            .navigationTitle("Connect Wallet")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { isPresented = false }
                }
            }
        }
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
    }
}
