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

            // ADR-0048 D6 (generalises V-14 / #963): remote-signer health row.
            // Only rendered when a remote-signer session is active
            // (`signerState` is nil for local-key-only accounts). Covers BOTH
            // NIP-46 bunker sessions and NIP-55 (Amber) sessions — the row
            // header is picked from `signerKind`. `isReady` is the happy path;
            // `isAwaitingApproval`/`isReconnecting` prompt the user to wait;
            // `isUnavailable`/`isFailed` prompt re-authentication. Rust
            // pre-computes every flag (ADR-0032 pattern).
            if let signerState = model.signerState {
                Section(signerState.signerKind == "nip55" ? "External signer" : "Signer relay") {
                    SignerStateRow(signerState: signerState)
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
                model.addSigner(localNsec: nsec.trimmingCharacters(in: .whitespacesAndNewlines), makeActive: true)
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
                model.addSigner(bunkerUri: trimmed, makeActive: true)
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

// MARK: - SignerStateRow (ADR-0048 D6, generalises V-14 / #963)

/// Shows the live health of the active remote-signer session — NIP-46 bunker
/// relay state or NIP-55 (Amber) external-signer state.
///
/// Only rendered when `model.signerState` is non-nil (i.e. a remote signer is
/// active). Rust pre-computes `isReady` / `isAwaitingApproval` /
/// `isReconnecting` / `isUnavailable` / `isFailed` (NIP-46: relay socket state
/// from `nmp-signer-broker`; NIP-55: Intent/ContentResolver outcomes). Swift
/// renders verbatim (ADR-0032 / relay_diagnostics pattern): no string-compare
/// on `state`.
///
/// The states drive distinct UX:
/// - `isReady` → green `circle.fill` + "Connected" label.
/// - `isAwaitingApproval` → spinner + "Waiting for approval…". A NIP-55 Intent
///   round-trip is in flight; the user should approve in the signer app.
/// - `isReconnecting` → spinner + "Reconnecting…". The user should wait;
///   auto-reconnect is in progress. `reason` surfaced as secondary text.
/// - `isUnavailable` → red `exclamationmark.triangle.fill` + "Signer
///   unavailable". The signer app is missing; the user should reinstall or
///   re-authenticate. `reason` surfaced as secondary text.
/// - `isFailed` → red `exclamationmark.triangle.fill` + "Connection failed".
///   The session is bricked; the user should re-authenticate. `reason`
///   surfaced as secondary text.
private struct SignerStateRow: View {
    let signerState: SignerState

    /// Degraded-terminal grouping: both `unavailable` and `failed` render the
    /// red prompt-re-auth treatment.
    private var isDegradedTerminal: Bool {
        signerState.isFailed || signerState.isUnavailable
    }

    /// Transient in-progress grouping: both `awaiting_approval` and
    /// `reconnecting` render the live spinner.
    private var isInProgress: Bool {
        signerState.isAwaitingApproval || signerState.isReconnecting
    }

    private var statusIcon: String {
        if isDegradedTerminal { return "exclamationmark.triangle.fill" }
        if isInProgress { return "arrow.clockwise.circle.fill" }
        return "circle.fill"
    }

    private var statusColor: Color {
        if isDegradedTerminal { return ChirpColor.danger }
        if isInProgress { return ChirpColor.warning }
        return ChirpColor.success
    }

    private var statusLabel: String {
        if signerState.isUnavailable { return "Signer unavailable" }
        if signerState.isFailed { return "Connection failed" }
        if signerState.isAwaitingApproval { return "Waiting for approval…" }
        if signerState.isReconnecting { return "Reconnecting…" }
        return "Connected"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 8) {
                if isInProgress {
                    // Use an animated spinner for the transient states so the
                    // user can see live progress.
                    ProgressView()
                        .controlSize(.small)
                        .tint(statusColor)
                } else {
                    Image(systemName: statusIcon)
                        .foregroundStyle(statusColor)
                        .imageScale(.small)
                }
                Text(statusLabel)
                    .foregroundStyle(isDegradedTerminal
                                     ? ChirpColor.danger
                                     : ChirpColor.textPrimary)
            }
            if let reason = signerState.reason, !reason.isEmpty {
                Text(reason)
                    .font(.caption)
                    .foregroundStyle(isDegradedTerminal
                                     ? ChirpColor.danger
                                     : ChirpColor.textSecondary)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Signer: \(statusLabel)")
        .accessibilityIdentifier("signer-state-row")
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
        // projection-provided stage label. A current kernel supplies a non-empty
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
