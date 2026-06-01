import SwiftUI

struct AccountsView: View {
    @EnvironmentObject private var model: KernelModel
    @State private var showAddSheet = false

    var body: some View {
        List {
            if model.accounts.isEmpty {
                Section {
                    ContentUnavailableView(
                        "No accounts",
                        systemImage: "person.2.fill",
                        description: Text("Add or create an identity to get started.")
                    )
                }
            } else {
                Section("Identities") {
                    ForEach(model.accounts) { account in
                        AccountRowView(account: account)
                            .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                                Button(role: .destructive) {
                                    model.removeAccount(account.id)
                                } label: {
                                    Label("Remove", systemImage: "trash")
                                }
                            }
                    }
                }
            }

            Section {
                Button {
                    showAddSheet = true
                } label: {
                    Label("Add account", systemImage: "plus.circle.fill")
                }
            }
        }
        .navigationTitle("Accounts")
        .sheet(isPresented: $showAddSheet) {
            AddAccountSheet()
        }
    }
}

private struct AccountRowView: View {
    let account: AccountSummary
    @EnvironmentObject private var model: KernelModel

    var body: some View {
        Button {
            if !account.isActive {
                model.switchActive(account.id)
            }
        } label: {
            HStack {
                ChirpAvatar(
                    pubkey: account.id,
                    url: account.pictureUrl,
                    initials: (account.displayName ?? account.id).displayInitials,
                    colorHex: account.id.pubkeyColorHex,
                    size: 48
                )

                VStack(alignment: .leading, spacing: 2) {
                    Text(account.displayName?.isEmpty == false ? account.displayName! : "Identity")
                        .foregroundStyle(ChirpColor.textPrimary)
                        .lineLimit(1)
                    // ADR-0032 — shell-side bech32 abbreviation.
                    Text(account.npub.shortHex)
                        .font(.footnote.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                Spacer()

                Text(account.signerLabel)
                    .font(.caption2)
                    .foregroundStyle(.secondary)

                if account.isActive {
                    Image(systemName: "checkmark")
                        .foregroundStyle(ChirpColor.success)
                }
            }
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier(account.isActive ? "account-row-active" : "account-row-\(account.id)")
        .accessibilityValue(account.npub)
    }
}

private struct AddAccountSheet: View {
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    @State private var nsec = ""
    @State private var bunkerURI = ""
    @State private var selectedTab = 0
    @State private var bunkerSubmitted = false
    @State private var initialRemoteSignerIds: Set<String> = []
    @State private var detectedSignerApp: Nip46Onboarding.SignerApp? = nil

    var body: some View {
        NavigationStack {
            Form {
                Picker("Method", selection: $selectedTab) {
                    Text("Import key").tag(0)
                    Text("Bunker").tag(1)
                    Text("New identity").tag(2)
                }
                .pickerStyle(.segmented)

                switch selectedTab {
                case 0:
                    importKeySection
                case 1:
                    bunkerSection
                default:
                    newIdentitySection
                }
            }
            .navigationTitle("Add Account")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                }
            }
            .onAppear {
                initialRemoteSignerIds = Set(
                    model.accounts
                        .filter(\.signerIsRemote)
                        .map(\.id)
                )
                detectSignerApps()
            }
            .onChange(of: model.nip46Onboarding?.signerApps) { _, _ in
                detectSignerApps()
            }
            .onChange(of: model.accounts) { _, newValue in
                guard bunkerSubmitted else { return }
                let arrivedRemote = newValue.first { account in
                    account.signerIsRemote && !initialRemoteSignerIds.contains(account.id)
                }
                if arrivedRemote != nil {
                    bunkerSubmitted = false
                    bunkerURI = ""
                    dismiss()
                }
            }
        }
    }

    private var importKeySection: some View {
        Section {
            SecureField("nsec1…", text: $nsec)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()

            Button {
                model.signInNsec(nsec.trimmingCharacters(in: .whitespacesAndNewlines))
                dismiss()
            } label: {
                Label("Sign in", systemImage: "key.fill")
            }
            .disabled(nsec.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        } header: {
            Text("Private key")
        }
    }

    private var bunkerSection: some View {
        Section {
            if let signer = detectedSignerApp {
                Button {
                    loginWithDetectedSigner()
                } label: {
                    Label("Login with \(signer.displayLabel)", systemImage: "arrow.up.forward.app")
                }
                .disabled(isHandshakeInFlight)
            }

            HStack {
                Image(systemName: "network")
                    .foregroundStyle(.secondary)
                TextField("bunker://…", text: $bunkerURI)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .disabled(isHandshakeInFlight)
            }

            // Doctrine §6 anti-pattern #1: the visibility guard reads the
            // pre-computed `isIdle` flag instead of `.lowercased() == "idle"`.
            // The actor maps an `"idle"` stage to `None` (clearing the slot
            // and `model.bunkerHandshake` to `nil`), so this branch defends
            // against a future broker path that emits `"idle"` straight into
            // the projection without going through `bunker_handshake_progress`.
            // The `?? false` fallback covers legacy kernels (D1) that emit
            // the projection without the new flags.
            if bunkerSubmitted, let handshake = model.bunkerHandshake,
               !(handshake.isIdle ?? false) {
                BunkerHandshakeProgress(
                    handshake: handshake,
                    onCancel: cancelHandshake
                )
            }

            Button {
                let trimmed = bunkerURI.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !trimmed.isEmpty else { return }
                bunkerSubmitted = true
                model.signInBunker(trimmed)
            } label: {
                Label(connectButtonTitle, systemImage: "network")
            }
            .disabled(isConnectDisabled)
        } header: {
            Text("Remote signer")
        }
    }

    private func detectSignerApps() {
        guard let signerApps = model.nip46Onboarding?.signerApps else {
            detectedSignerApp = nil
            return
        }
        detectedSignerApp = signerApps.first { app in
            URL(string: app.scheme).map { UIApplication.shared.canOpenURL($0) } ?? false
        }
    }

    private func loginWithDetectedSigner() {
        guard let uri = model.nostrConnectURI(), let url = URL(string: uri) else {
            return
        }
        bunkerSubmitted = true
        UIApplication.shared.open(url)
    }

    private var trimmedBunkerURI: String {
        bunkerURI.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var isHandshakeInFlight: Bool {
        guard bunkerSubmitted else { return false }
        // Doctrine §6 anti-pattern #1: read the pre-computed `isInFlight` flag
        // instead of reconstructing the rule from `stage` string comparisons.
        // No handshake yet (nil) means we just submitted and are waiting on the
        // first progress tick — treat it as in-flight so the button stays
        // disabled. The `?? false` covers a legacy kernel that emits the
        // projection without the new flag (D1: fall back to stage parsing
        // would also work but the conservative default is harmless).
        guard let handshake = model.bunkerHandshake else { return true }
        return handshake.isInFlight ?? false
    }

    private var isConnectDisabled: Bool {
        trimmedBunkerURI.isEmpty || isHandshakeInFlight
    }

    private var connectButtonTitle: String {
        // Doctrine §6 anti-pattern #1: read the pre-computed `isFailed` flag
        // instead of `.lowercased() == "failed"`.
        if model.bunkerHandshake?.isFailed ?? false {
            return "Retry"
        }
        return "Connect"
    }

    private func cancelHandshake() {
        model.cancelBunkerHandshake()
        bunkerSubmitted = false
    }

    private var newIdentitySection: some View {
        Section {
            Text("Create a brand new Nostr identity. A new keypair will be generated for you. Make sure to back it up later from Settings → Accounts.")
                .font(.footnote)
                .foregroundStyle(.secondary)

            Button {
                model.createAccount(profile: ["name": "New User"])
                dismiss()
            } label: {
                Label("Create new identity", systemImage: "sparkles")
            }
            .accessibilityIdentifier("create-new-identity-button")
        } header: {
            Text("Fresh start")
        }
    }
}

private struct BunkerHandshakeProgress: View {
    let handshake: BunkerHandshake
    let onCancel: () -> Void

    // Doctrine §6 anti-pattern #1 / RMP bible commandment #4: every derived
    // value below comes from Rust (`BunkerHandshakeDto::new`). The Swift
    // helpers used to switch on `handshake.stage.lowercased()` — that ternary
    // tree now lives in `crates/nmp-core/src/actor/commands/identity.rs`.

    private var isFailed: Bool {
        handshake.isFailed ?? false
    }

    private var isTerminal: Bool {
        // A handshake is "terminal" when it has either succeeded or failed.
        // Rust pre-computes both flags; their disjunction is the visibility
        // gate for the "cancel" button and the icon-swap.
        (handshake.isTerminalSuccess ?? false) || (handshake.isFailed ?? false)
    }

    private var stageLabel: String {
        // Fall back to `stage` only for legacy kernels (D1) that predate the
        // pre-formatted label. A current kernel always supplies a non-empty
        // `stageLabel`, so this fallback never fires in production today.
        handshake.stageLabel ?? handshake.stage
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                if isTerminal {
                    Image(systemName: isFailed
                          ? "exclamationmark.triangle.fill"
                          : "checkmark.circle.fill")
                        .foregroundStyle(isFailed ? ChirpColor.danger : ChirpColor.success)
                } else {
                    ProgressView()
                        .controlSize(.small)
                }
                Text(stageLabel)
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 0)
            }

            if let message = handshake.message, !message.isEmpty {
                Text(message)
                    .font(.caption)
                    .foregroundStyle(isFailed ? ChirpColor.danger : ChirpColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            if !isTerminal {
                Button("Cancel handshake", action: onCancel)
                    .font(.caption)
            }
        }
        .padding(.vertical, 4)
    }
}
