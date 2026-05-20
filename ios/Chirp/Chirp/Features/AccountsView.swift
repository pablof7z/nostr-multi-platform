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

    private var isActive: Bool {
        model.activeAccount == account.id
    }

    var body: some View {
        Button {
            if !isActive {
                model.switchActive(account.id)
            }
        } label: {
            HStack {
                ChirpAvatar(
                    url: nil,
                    initials: account.avatarInitials,
                    colorHex: account.avatarColorHex,
                    size: 48
                )

                VStack(alignment: .leading, spacing: 2) {
                    Text(account.displayName.isEmpty ? "Identity" : account.displayName)
                        .lineLimit(1)
                    Text(shortNpub(account.npub))
                        .font(.footnote.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                Spacer()

                Text(signerLabel(account.signerKind))
                    .font(.caption2)
                    .foregroundStyle(.secondary)

                if isActive {
                    Image(systemName: "checkmark")
                        .foregroundStyle(Color.accentColor)
                }
            }
        }
        .accessibilityIdentifier(isActive ? "account-row-active" : "account-row-\(account.id)")
        .accessibilityValue(account.npub)
    }

    private func shortNpub(_ npub: String) -> String {
        guard npub.count >= 16 else { return npub }
        return "\(npub.prefix(10))…\(npub.suffix(6))"
    }

    private func signerLabel(_ kind: String) -> String {
        switch kind.lowercased() {
        case "nsec": return "nsec"
        case "bunker", "nip46": return "NIP-46"
        default: return kind
        }
    }
}

private struct AddAccountSheet: View {
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    @State private var nsec = ""
    @State private var bunkerURI = ""
    @State private var selectedTab = 0
    @State private var bunkerSubmitted = false
    @State private var initialNip46Ids: Set<String> = []

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
                initialNip46Ids = Set(
                    model.accounts
                        .filter { $0.signerKind.lowercased() == "nip46" }
                        .map(\.id)
                )
            }
            .onChange(of: model.accounts) { _, newValue in
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
            HStack {
                Image(systemName: "network")
                    .foregroundStyle(.secondary)
                TextField("bunker://…", text: $bunkerURI)
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
            Text("Bunker URI")
        }
    }

    private var trimmedBunkerURI: String {
        bunkerURI.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var isHandshakeInFlight: Bool {
        guard bunkerSubmitted else { return false }
        guard let stage = model.bunkerHandshake?.stage.lowercased() else { return true }
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

    private var newIdentitySection: some View {
        Section {
            Text("Create a brand new Nostr identity. A new keypair will be generated for you. Make sure to back it up later from Settings → Accounts.")
                .font(.footnote)
                .foregroundStyle(.secondary)

            Button {
                model.createAccount(profile: ["name": "New User"], relays: [
                    ("wss://relay.primal.net", "both,indexer"),
                    ("wss://purplepag.es", "indexer"),
                ])
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

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                if isTerminal {
                    Image(systemName: isFailed
                          ? "exclamationmark.triangle.fill"
                          : "checkmark.circle.fill")
                        .foregroundStyle(isFailed ? .red : .green)
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
                    .foregroundStyle(isFailed ? .red : .secondary)
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
