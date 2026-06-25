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
                    .font(.system(size: 56, weight: .light))
                    .foregroundStyle(ChirpColor.accent)
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
                .buttonBorderShape(.roundedRectangle(radius: 12))
                .controlSize(.large)
                .tint(ChirpColor.accent)
                .frame(maxWidth: 260)
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
                    // ADR-0032 / #623: bind pre-computed label and tone — no
                    // protocol-string branching in Swift (thin-shell rule).
                    HStack(spacing: 5) {
                        Circle()
                            .fill(color(for: status.statusTone))
                            .frame(width: 6, height: 6)
                        Text(status.statusLabel)
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(color(for: status.statusTone))
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
                    .foregroundStyle(ChirpColor.accent)
                    .symbolRenderingMode(.hierarchical)

                if let sats = status.balanceSats {
                    Text("\(sats.formatted(.number)) sats")
                        .font(.largeTitle.weight(.bold))
                } else {
                    // `isReady == false` when connecting; the Rust projection
                    // pre-computed this — no raw-string compare needed here.
                    Text(status.isReady ? "— sats" : "Fetching balance…")
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

    /// Map a pre-computed `statusTone` string to a `Color`.
    ///
    /// The tone vocabulary is `"active"` | `"warning"` | `"error"` |
    /// `"inactive"` (ADR-0032 / #623). No other protocol-string knowledge
    /// lives here — the Rust projection owns the mapping from wire status to
    /// tone (thin-shell rule).
    private func color(for tone: String) -> Color {
        switch tone {
        case "active":   return ChirpColor.success
        case "warning":  return ChirpColor.zap
        case "error":    return ChirpColor.danger
        default:         return ChirpColor.textSecondary
        }
    }

    // ── Wallet actions ────────────────────────────────────────────────────────

    private func walletActionsSection(status: WalletStatusData) -> some View {
        Section("Actions") {
            HStack(spacing: 12) {
                Image(systemName: "arrow.up.circle.fill")
                    .foregroundStyle(ChirpColor.accent)
                Text("Send")
                    .font(.callout.weight(.semibold))
                Spacer()
                Text("Paste invoice to pay")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    // A former "Powered By" section rendered three candy-colored protocol
    // tiles (NWC / NIP-57 / Cashu). It was decorative marketing chrome — the
    // disconnected copy already names NWC/Alby/Zeus — and read as amateur on
    // an otherwise restrained wallet screen, so it was removed (iOS polish
    // pass, docs/design/ios-polish-checklist.md).

    // ADR-0032 / #623: the former P5 violations (raw-status `.capitalized`,
    // `== "connecting"` branch, `statusColor` switch on wire strings) were
    // eliminated; `statusLabel` / `statusTone` are derived locally from the
    // raw `status` token (see `WalletStatusTone`).
    //
    // #1678 / D7: `wallet_npub_short` was removed from the wire. The
    // connected section uses `status.walletPubkeyHex.shortHex` (a Swift
    // shell-side abbreviation of the raw 64-char hex field).
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
                    .buttonBorderShape(.roundedRectangle(radius: 12))
                    .tint(ChirpColor.accent)
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
