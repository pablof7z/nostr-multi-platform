import SwiftUI

// OWNER: Phase-2 Agent C (Accounts / multi-session). Replace whole file.

struct AccountsView: View {
    @EnvironmentObject private var model: KernelModel
    @State private var showAddSheet = false

    var body: some View {
        List {
            if model.accounts.isEmpty {
                Section {
                    ChirpPlaceholder(
                        systemImage: "person.2.fill",
                        title: "No accounts",
                        subtitle: "Add or create an identity to get started."
                    )
                    .frame(maxWidth: .infinity)
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
                }
            } else {
                Section {
                    ForEach(model.accounts) { account in
                        AccountRowView(account: account)
                            .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                                Button(role: .destructive) {
                                    model.removeAccount(account.id)
                                } label: {
                                    Label("Remove", systemImage: "trash")
                                }
                            }
                            .listRowBackground(Color.clear)
                            .listRowSeparator(.hidden)
                    }
                } header: {
                    ChirpSectionHeader(title: "Identities")
                        .padding(.bottom, ChirpSpace.xs)
                }
            }

            // Add account button row
            Section {
                Button {
                    showAddSheet = true
                } label: {
                    Label("Add account", systemImage: "plus.circle.fill")
                        .font(ChirpFont.headline)
                        .foregroundStyle(ChirpColor.accent)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.vertical, ChirpSpace.s)
                }
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
            }
        }
        .listStyle(.plain)
        .background(Color(.systemBackground))
        .navigationTitle("Accounts")
        .sheet(isPresented: $showAddSheet) {
            AddAccountSheet()
        }
    }
}

// ── Account row ───────────────────────────────────────────────────────────

private struct AccountRowView: View {
    let account: AccountSummary
    @EnvironmentObject private var model: KernelModel

    private var isActive: Bool {
        model.activeAccount == account.id
    }

    var body: some View {
        Button {
            if !isActive {
                model.switchActive(account.id)
            }
        } label: {
            HStack(spacing: ChirpSpace.m) {
                ChirpAvatar(
                    url: nil,
                    initials: account.avatarInitials,
                    colorHex: account.avatarColorHex,
                    size: 48
                )

                VStack(alignment: .leading, spacing: ChirpSpace.xs) {
                    Text(account.displayName.isEmpty ? "Identity" : account.displayName)
                        .font(ChirpFont.headline)
                        .foregroundStyle(ChirpColor.textPrimary)
                        .lineLimit(1)

                    Text(shortNpub(account.npub))
                        .font(ChirpFont.mono)
                        .foregroundStyle(ChirpColor.textTertiary)
                        .lineLimit(1)
                }

                Spacer()

                // Signer kind badge
                SignerBadge(kind: account.signerKind)

                // Active checkmark
                if isActive {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.system(size: 20, weight: .semibold))
                        .foregroundStyle(ChirpColor.accent)
                        .transition(.scale.combined(with: .opacity))
                }
            }
            .padding(.vertical, ChirpSpace.s)
            .padding(.horizontal, ChirpSpace.m)
            .background(
                isActive
                    ? ChirpColor.accentSoft
                    : Color(.secondarySystemBackground).opacity(0.6),
                in: RoundedRectangle(cornerRadius: ChirpSpace.radiusSmall, style: .continuous)
            )
            .overlay(
                RoundedRectangle(cornerRadius: ChirpSpace.radiusSmall, style: .continuous)
                    .strokeBorder(
                        isActive ? ChirpColor.accent.opacity(0.3) : ChirpColor.hairline,
                        lineWidth: 1
                    )
            )
            .animation(.smooth(duration: 0.2), value: isActive)
        }
        .buttonStyle(.plain)
    }

    private func shortNpub(_ npub: String) -> String {
        guard npub.count >= 16 else { return npub }
        return "\(npub.prefix(10))…\(npub.suffix(6))"
    }
}

// ── Signer kind badge ─────────────────────────────────────────────────────

private struct SignerBadge: View {
    let kind: String

    private var label: String {
        switch kind.lowercased() {
        case "nsec": return "nsec"
        case "bunker", "nip46": return "NIP-46"
        default: return kind
        }
    }

    private var icon: String {
        switch kind.lowercased() {
        case "nsec": return "key.fill"
        case "bunker", "nip46": return "network"
        default: return "person.crop.circle"
        }
    }

    var body: some View {
        HStack(spacing: 3) {
            Image(systemName: icon)
                .font(.system(size: 10, weight: .semibold))
            Text(label)
                .font(.system(.caption2, design: .rounded).weight(.semibold))
        }
        .foregroundStyle(ChirpColor.accent)
        .padding(.horizontal, ChirpSpace.s)
        .padding(.vertical, 4)
        .background(ChirpColor.accentSoft, in: Capsule())
    }
}

// ── Add account sheet ─────────────────────────────────────────────────────

private struct AddAccountSheet: View {
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    @State private var nsec = ""
    @State private var bunkerURI = ""
    @State private var selectedTab = 0
    /// Set to `true` once the user taps "Connect" on the bunker tab. Drives
    /// the disabled / progress states. Reset on success, failure-retry, or
    /// manual cancel.
    @State private var bunkerSubmitted = false
    /// NIP-46 account ids present when the sheet opened. A newly-arrived
    /// `signer_kind == "nip46"` account whose id isn't in this set is
    /// treated as a successful handshake → dismiss.
    @State private var initialNip46Ids: Set<String> = []

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: ChirpSpace.xl) {
                    // Tab picker: Import / Bunker / New
                    Picker("Method", selection: $selectedTab) {
                        Text("Import key").tag(0)
                        Text("Bunker").tag(1)
                        Text("New identity").tag(2)
                    }
                    .pickerStyle(.segmented)
                    .padding(.horizontal, ChirpSpace.l)

                    switch selectedTab {
                    case 0:
                        importKeySection
                    case 1:
                        bunkerSection
                    default:
                        newIdentitySection
                    }
                }
                .padding(.top, ChirpSpace.l)
            }
            .background(Color(.systemBackground))
            .navigationTitle("Add Account")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                        .foregroundStyle(ChirpColor.textSecondary)
                }
            }
            .onAppear {
                initialNip46Ids = Set(
                    model.accounts
                        .filter { $0.signerKind.lowercased() == "nip46" }
                        .map(\.id)
                )
            }
            .onChange(of: model.accounts) { _, newValue in
                // Auto-dismiss on successful bunker handshake: any nip46
                // account whose id wasn't present when the sheet opened.
                guard bunkerSubmitted else { return }
                let arrivedNip46 = newValue.first { account in
                    account.signerKind.lowercased() == "nip46"
                        && !initialNip46Ids.contains(account.id)
                }
                if arrivedNip46 != nil {
                    bunkerSubmitted = false
                    bunkerURI = ""
                    dismiss()
                }
            }
        }
    }

    // ── Import nsec ───────────────────────────────────────────────────────

    private var importKeySection: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: ChirpSpace.m) {
                ChirpSectionHeader(title: "Private key")

                SecureField("nsec1…", text: $nsec)
                    .font(ChirpFont.mono)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()

                ChirpPrimaryButton(
                    title: "Sign in",
                    systemImage: "key.fill"
                ) {
                    model.signInNsec(nsec.trimmingCharacters(in: .whitespacesAndNewlines))
                    dismiss()
                }
                .disabled(nsec.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                .opacity(nsec.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? 0.45 : 1.0)
            }
        }
        .padding(.horizontal, ChirpSpace.l)
    }

    // ── Bunker (NIP-46) ──────────────────────────────────────────────────
    //
    // Live progress is mirrored from `model.bunkerHandshake`, which the
    // kernel populates from snapshot field `bunker_handshake` (Stage 3
    // backend). Stage 4 broker is what actually drives the stage
    // transitions; this view is read-only until the user retries.

    private var bunkerSection: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: ChirpSpace.m) {
                ChirpSectionHeader(title: "Bunker URI")

                HStack(spacing: ChirpSpace.s) {
                    Image(systemName: "network")
                        .foregroundStyle(ChirpColor.accent)
                        .font(.system(size: 15))
                    TextField("bunker://…", text: $bunkerURI)
                        .font(ChirpFont.mono)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .disabled(isHandshakeInFlight)
                }

                if bunkerSubmitted, let handshake = model.bunkerHandshake,
                   handshake.stage.lowercased() != "idle" {
                    BunkerHandshakeProgress(
                        handshake: handshake,
                        onCancel: cancelHandshake
                    )
                }

                ChirpPrimaryButton(
                    title: connectButtonTitle,
                    systemImage: "network"
                ) {
                    let trimmed = bunkerURI.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !trimmed.isEmpty else { return }
                    bunkerSubmitted = true
                    model.signInBunker(trimmed)
                }
                .disabled(isConnectDisabled)
                .opacity(isConnectDisabled ? 0.45 : 1.0)
            }
        }
        .padding(.horizontal, ChirpSpace.l)
    }

    // ── Bunker helpers ───────────────────────────────────────────────────

    private var trimmedBunkerURI: String {
        bunkerURI.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// True while we're waiting for the broker — i.e. the user has submitted
    /// AND the snapshot reports a non-terminal stage. `"failed"` re-enables
    /// the form so the user can retry.
    private var isHandshakeInFlight: Bool {
        guard bunkerSubmitted else { return false }
        guard let stage = model.bunkerHandshake?.stage.lowercased() else {
            // Submitted but no snapshot yet — treat as in-flight so the
            // user can't double-submit before the kernel responds.
            return true
        }
        return stage != "failed" && stage != "idle"
    }

    private var isConnectDisabled: Bool {
        trimmedBunkerURI.isEmpty || isHandshakeInFlight
    }

    private var connectButtonTitle: String {
        if model.bunkerHandshake?.stage.lowercased() == "failed" {
            return "Retry"
        }
        return "Connect"
    }

    private func cancelHandshake() {
        model.cancelBunkerHandshake()
        bunkerSubmitted = false
    }

    // ── Create new identity ───────────────────────────────────────────────

    private var newIdentitySection: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: ChirpSpace.m) {
                ChirpSectionHeader(title: "Fresh start")

                VStack(alignment: .leading, spacing: ChirpSpace.s) {
                    Text("Create a brand new Nostr identity.")
                        .font(ChirpFont.callout)
                        .foregroundStyle(ChirpColor.textSecondary)
                    Text("A new keypair will be generated for you. Make sure to back it up later from Settings → Accounts.")
                        .font(ChirpFont.caption)
                        .foregroundStyle(ChirpColor.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }

                ChirpPrimaryButton(
                    title: "Create new identity",
                    systemImage: "sparkles"
                ) {
                    model.createAccount()
                    dismiss()
                }
            }
        }
        .padding(.horizontal, ChirpSpace.l)
    }
}

// ── Bunker handshake progress UI ──────────────────────────────────────────

/// Live progress block shown beneath the bunker URI field while a NIP-46
/// handshake is in flight. Mirrors the `bunker_handshake` snapshot field.
private struct BunkerHandshakeProgress: View {
    let handshake: BunkerHandshake
    let onCancel: () -> Void

    private var isFailed: Bool {
        handshake.stage.lowercased() == "failed"
    }

    private var isTerminal: Bool {
        let stage = handshake.stage.lowercased()
        return stage == "ready" || stage == "failed"
    }

    private var stageLabel: String {
        switch handshake.stage.lowercased() {
        case "connecting": return "Connecting to bunker relays…"
        case "awaiting_pubkey": return "Awaiting bunker approval…"
        case "ready": return "Connected"
        case "failed": return "Bunker handshake failed"
        default: return handshake.stage
        }
    }

    private var accent: Color {
        isFailed ? ChirpColor.like : ChirpColor.accent
    }

    var body: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.s) {
            HStack(spacing: ChirpSpace.s) {
                if isTerminal {
                    Image(systemName: isFailed
                          ? "exclamationmark.triangle.fill"
                          : "checkmark.circle.fill")
                        .foregroundStyle(accent)
                        .font(.system(size: 14, weight: .semibold))
                } else {
                    ProgressView()
                        .progressViewStyle(.circular)
                        .controlSize(.small)
                        .tint(accent)
                }
                Text(stageLabel)
                    .font(ChirpFont.callout)
                    .foregroundStyle(ChirpColor.textPrimary)
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 0)
            }

            if let message = handshake.message, !message.isEmpty {
                Text(message)
                    .font(ChirpFont.caption)
                    .foregroundStyle(isFailed ? ChirpColor.like : ChirpColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            if !isTerminal {
                Button(action: onCancel) {
                    Text("Cancel handshake")
                        .font(ChirpFont.caption.weight(.semibold))
                        .foregroundStyle(ChirpColor.textSecondary)
                }
                .buttonStyle(.plain)
                .padding(.top, ChirpSpace.xs)
            }
        }
        .padding(ChirpSpace.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            (isFailed ? ChirpColor.like.opacity(0.10) : ChirpColor.accentSoft),
            in: RoundedRectangle(cornerRadius: ChirpSpace.radiusSmall, style: .continuous)
        )
        .overlay(
            RoundedRectangle(cornerRadius: ChirpSpace.radiusSmall, style: .continuous)
                .strokeBorder(accent.opacity(0.25), lineWidth: 1)
        )
        .animation(.smooth(duration: 0.2), value: handshake.stage)
    }
}
