import SwiftUI

struct WalletView: View {
    @EnvironmentObject private var model: KernelModel
    @State private var showingConnectSheet = false

    var body: some View {
        ScrollView {
            VStack(spacing: ChirpSpace.xl) {
                if let status = model.walletStatus, status.isConnected {
                    connectedCard(status: status)
                    if status.isReady {
                        walletActions(status: status)
                    }
                } else {
                    disconnectedCard
                }
                technologyCards
            }
            .padding(.horizontal, ChirpSpace.l)
            .padding(.top, ChirpSpace.m)
            .padding(.bottom, ChirpSpace.xxl)
        }
        .background(Color(.systemBackground))
        .navigationTitle("Wallet")
        .navigationBarTitleDisplayMode(.large)
        .sheet(isPresented: $showingConnectSheet) {
            ConnectWalletSheet(isPresented: $showingConnectSheet)
                .environmentObject(model)
        }
    }

    // ── Disconnected state ────────────────────────────────────────────────────

    private var disconnectedCard: some View {
        VStack(spacing: ChirpSpace.xl) {
            ZStack {
                RoundedRectangle(cornerRadius: ChirpSpace.radius, style: .continuous)
                    .fill(
                        LinearGradient(
                            colors: [
                                ChirpColor.zap.opacity(0.12),
                                ChirpColor.accent.opacity(0.08),
                                Color(.systemBackground).opacity(0)
                            ],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        )
                    )
                RoundedRectangle(cornerRadius: ChirpSpace.radius, style: .continuous)
                    .strokeBorder(ChirpColor.zap.opacity(0.2), lineWidth: 1)

                VStack(spacing: ChirpSpace.xl) {
                    Image(systemName: "bolt.circle")
                        .font(.system(size: 52, weight: .light))
                        .foregroundStyle(
                            LinearGradient(
                                colors: [ChirpColor.zap, ChirpColor.zap.opacity(0.5)],
                                startPoint: .top, endPoint: .bottom
                            )
                        )
                        .symbolRenderingMode(.hierarchical)

                    VStack(spacing: ChirpSpace.s) {
                        Text("Connect a Wallet")
                            .font(ChirpFont.title)
                            .foregroundStyle(ChirpColor.textPrimary)
                        Text("Use any NWC-compatible wallet — Alby, Zeus, Mutiny, or self-hosted — to send and receive Lightning payments.")
                            .font(ChirpFont.callout)
                            .foregroundStyle(ChirpColor.textSecondary)
                            .multilineTextAlignment(.center)
                    }

                    Button {
                        showingConnectSheet = true
                    } label: {
                        Label("Connect Wallet", systemImage: "bolt.fill")
                            .font(ChirpFont.callout.weight(.semibold))
                            .foregroundStyle(.white)
                            .padding(.horizontal, ChirpSpace.xl)
                            .padding(.vertical, ChirpSpace.m)
                            .background(ChirpColor.zap, in: Capsule())
                    }
                }
                .padding(ChirpSpace.xl)
            }
        }
    }

    // ── Connected state ───────────────────────────────────────────────────────

    private func connectedCard(status: WalletStatusData) -> some View {
        ZStack {
            RoundedRectangle(cornerRadius: ChirpSpace.radius, style: .continuous)
                .fill(
                    LinearGradient(
                        colors: [
                            ChirpColor.zap.opacity(0.18),
                            ChirpColor.accent.opacity(0.12),
                            Color(.systemBackground).opacity(0)
                        ],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )
            RoundedRectangle(cornerRadius: ChirpSpace.radius, style: .continuous)
                .strokeBorder(ChirpColor.zap.opacity(0.3), lineWidth: 1)

            VStack(spacing: ChirpSpace.l) {
                HStack {
                    statusBadge(status: status.status)
                    Spacer()
                    Button(role: .destructive) {
                        model.walletDisconnect()
                    } label: {
                        Text("Disconnect")
                            .font(ChirpFont.caption.weight(.medium))
                            .foregroundStyle(ChirpColor.textTertiary)
                    }
                }

                Image(systemName: "bolt.circle.fill")
                    .font(.system(size: 48, weight: .light))
                    .foregroundStyle(
                        LinearGradient(
                            colors: [ChirpColor.zap, ChirpColor.zap.opacity(0.6)],
                            startPoint: .top, endPoint: .bottom
                        )
                    )
                    .symbolRenderingMode(.hierarchical)

                if let sats = status.balanceSats {
                    Text("\(sats.formatted()) sats")
                        .font(.system(.largeTitle, design: .rounded).weight(.bold))
                        .foregroundStyle(ChirpColor.textPrimary)
                } else {
                    Text(status.status == "connecting" ? "Fetching balance…" : "— sats")
                        .font(.system(.largeTitle, design: .rounded).weight(.bold))
                        .foregroundStyle(ChirpColor.textTertiary)
                }

                Text(shortNpub(status.walletNpub))
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textTertiary)
                    .lineLimit(1)
            }
            .padding(ChirpSpace.xl)
        }
    }

    private func statusBadge(status: String) -> some View {
        HStack(spacing: 5) {
            Circle()
                .fill(statusColor(status))
                .frame(width: 6, height: 6)
            Text(status.capitalized)
                .font(.system(.caption2, design: .rounded).weight(.semibold))
                .foregroundStyle(statusColor(status))
        }
        .padding(.horizontal, ChirpSpace.s)
        .padding(.vertical, 4)
        .background(statusColor(status).opacity(0.12), in: Capsule())
    }

    private func statusColor(_ status: String) -> Color {
        switch status {
        case "ready": return ChirpColor.positive
        case "connecting": return ChirpColor.zap
        case "error": return .red
        default: return ChirpColor.textTertiary
        }
    }

    // ── Wallet actions ────────────────────────────────────────────────────────

    private func walletActions(status: WalletStatusData) -> some View {
        GlassCard {
            VStack(spacing: ChirpSpace.m) {
                ChirpSectionHeader(title: "Actions")
                Text("Pay Invoice")
                    .font(ChirpFont.callout)
                    .foregroundStyle(ChirpColor.textTertiary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                // Simple pay invoice button — opens paste-invoice sheet (future work)
                HStack(spacing: ChirpSpace.m) {
                    Label("Send", systemImage: "arrow.up.circle.fill")
                        .font(ChirpFont.callout.weight(.semibold))
                        .foregroundStyle(ChirpColor.zap)
                    Spacer()
                    Text("Paste invoice to pay")
                        .font(ChirpFont.caption)
                        .foregroundStyle(ChirpColor.textTertiary)
                }
            }
        }
    }

    // ── Technology cards ──────────────────────────────────────────────────────

    private var technologyCards: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Powered By")
            HStack(spacing: ChirpSpace.m) {
                TechPill(label: "NWC", sublabel: "Nostr Wallet Connect", color: ChirpColor.zap)
                TechPill(label: "NIP-57", sublabel: "Zap protocol", color: ChirpColor.accent)
                TechPill(label: "Cashu", sublabel: "Ecash tokens", color: Color(red: 0.85, green: 0.55, blue: 0.20))
            }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    private func shortNpub(_ npub: String) -> String {
        guard npub.count > 16 else { return npub }
        return "\(npub.prefix(10))…\(npub.suffix(6))"
    }
}

// ── Connect Wallet Sheet ───────────────────────────────────────────────────────

private struct ConnectWalletSheet: View {
    @EnvironmentObject private var model: KernelModel
    @Binding var isPresented: Bool
    @State private var uri = ""

    var body: some View {
        NavigationView {
            VStack(spacing: ChirpSpace.xl) {
                VStack(alignment: .leading, spacing: ChirpSpace.m) {
                    Text("Paste your NWC connection string from Alby, Zeus, Mutiny, or any NIP-47 compatible wallet.")
                        .font(ChirpFont.callout)
                        .foregroundStyle(ChirpColor.textSecondary)

                    TextEditor(text: $uri)
                        .font(.system(.body, design: .monospaced))
                        // NWC URIs are case-sensitive hex; the leading "n" must
                        // not be auto-capitalized or the parser's scheme check
                        // (and the Connect-button enable check) would fail.
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .frame(minHeight: 100)
                        .padding(ChirpSpace.m)
                        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 10))
                        .overlay(
                            RoundedRectangle(cornerRadius: 10)
                                .strokeBorder(Color(.separator), lineWidth: 1)
                        )

                    if uri.isEmpty {
                        Text("nostr+walletconnect://…")
                            .font(.system(.body, design: .monospaced))
                            .foregroundStyle(ChirpColor.textTertiary)
                            .padding(.leading, ChirpSpace.m)
                            .allowsHitTesting(false)
                    }
                }

                Button {
                    let trimmed = uri.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !trimmed.isEmpty else { return }
                    model.walletConnect(uri: trimmed)
                    isPresented = false
                } label: {
                    Label("Connect", systemImage: "bolt.fill")
                        .font(ChirpFont.callout.weight(.semibold))
                        .foregroundStyle(.white)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, ChirpSpace.m)
                        .background(
                            schemeLooksValid(uri) ? ChirpColor.zap : ChirpColor.textTertiary,
                            in: RoundedRectangle(cornerRadius: 12)
                        )
                }
                .disabled(!schemeLooksValid(uri))

                Spacer()
            }
            .padding(ChirpSpace.l)
            .navigationTitle("Connect Wallet")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { isPresented = false }
                }
                ToolbarItem(placement: .primaryAction) {
                    Button("Paste") {
                        if let pasted = UIPasteboard.general.string {
                            uri = pasted
                        }
                    }
                }
            }
        }
    }

    /// Case-insensitive scheme check. Auto-capitalize is disabled on the
    /// TextEditor, but paste sources (browser deeplinks, Notes/Mail apps) can
    /// still deliver `Nostr+walletconnect://`. The Rust parser also matches
    /// case-insensitively; this keeps the Connect button consistent with it.
    private func schemeLooksValid(_ s: String) -> Bool {
        let trimmed = s.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.lowercased().hasPrefix("nostr+walletconnect://")
    }
}

// ── Technology pill ────────────────────────────────────────────────────────────

private struct TechPill: View {
    let label: String
    let sublabel: String
    let color: Color

    var body: some View {
        GlassCard {
            VStack(spacing: ChirpSpace.xs) {
                Text(label)
                    .font(ChirpFont.headline)
                    .foregroundStyle(color)
                Text(sublabel)
                    .font(ChirpFont.caption)
                    .foregroundStyle(ChirpColor.textTertiary)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, ChirpSpace.xs)
        }
    }
}
