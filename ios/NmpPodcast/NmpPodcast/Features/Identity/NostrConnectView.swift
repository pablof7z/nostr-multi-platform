import CoreImage.CIFilterBuiltins
import SwiftUI
import UIKit

// MARK: - NostrConnectView
//
// Client-initiated NIP-46 pairing via nostrconnect://
//
// Shows a QR code the user scans in a signer app (Amber, nsec.app, etc.)
// or quick-launch buttons for detected installed signers.
// Fires `connectViaNostrConnect` on the identity store; the in-flight
// state is reflected by `remoteSignerState` on the environment store.

struct NostrConnectView: View {

    @Environment(UserIdentityStore.self) private var identity
    @Environment(AppStateStore.self) private var store
    @Environment(\.dismiss) private var dismiss

    @State private var nostrConnectURI: String = ""
    @State private var qrImage: UIImage?
    @State private var detectedSigners: [KnownSigner] = []
    @State private var pairingTask: Task<Void, Never>?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppTheme.Spacing.lg) {
                preface
                switch identity.remoteSignerState {
                case .connecting:
                    waitingSection
                case .connected:
                    connectedSection
                case .failed(let msg):
                    errorSection(msg)
                default:
                    setupSection
                }
            }
            .padding(AppTheme.Spacing.lg)
        }
        .navigationTitle("Scan to connect")
        .navigationBarTitleDisplayMode(.inline)
        .background(Color(.systemBackground))
        .toolbar {
            if !isPaired {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { cancelAndDismiss() }
                }
            }
        }
        .onAppear { beginPairing() }
        .onDisappear { pairingTask?.cancel() }
    }

    // MARK: - Sections

    private var preface: some View {
        VStack(alignment: .leading, spacing: AppTheme.Spacing.sm) {
            Text("Open your signer app, tap \u{201C}Scan\u{201D} or \u{201C}New connection\u{201D}, then point it at this code.")
                .font(AppTheme.Typography.body)
                .foregroundStyle(.primary)
            Text("Your private key never touches this device.")
                .font(AppTheme.Typography.caption)
                .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private var setupSection: some View {
        if let qrImage {
            qrSection(qrImage)
            if !detectedSigners.isEmpty {
                signerAppSection
            }
            footnote
        } else {
            ProgressView("Generating…")
                .frame(maxWidth: .infinity)
        }
    }

    private func qrSection(_ image: UIImage) -> some View {
        VStack(spacing: AppTheme.Spacing.md) {
            Image(uiImage: image)
                .interpolation(.none)
                .resizable()
                .scaledToFit()
                .frame(maxWidth: 260)
                .padding(AppTheme.Spacing.md)
                .background(Color.white, in: RoundedRectangle(cornerRadius: AppTheme.Corner.md))
                .frame(maxWidth: .infinity)
        }
    }

    private var signerAppSection: some View {
        VStack(alignment: .leading, spacing: AppTheme.Spacing.sm) {
            Text("Or open directly:")
                .font(AppTheme.Typography.caption2.weight(.semibold))
                .foregroundStyle(.tertiary)
                .textCase(.uppercase)
                .tracking(0.4)
            ForEach(detectedSigners, id: \.urlScheme) { signer in
                Button {
                    openSignerApp(signer)
                } label: {
                    HStack(spacing: AppTheme.Spacing.sm) {
                        Image(systemName: signer.systemImage)
                            .frame(width: 22)
                        Text("Open in \(signer.displayName)")
                        Spacer()
                        Image(systemName: "arrow.up.forward.app")
                            .font(.caption)
                            .foregroundStyle(.tertiary)
                    }
                    .padding(AppTheme.Spacing.sm)
                }
                .buttonStyle(.glass)
            }
        }
    }

    private var waitingSection: some View {
        VStack(spacing: AppTheme.Spacing.md) {
            if let qrImage {
                qrSection(qrImage).opacity(0.4)
            }
            HStack(spacing: AppTheme.Spacing.sm) {
                ProgressView().controlSize(.small)
                Text("Waiting for signer to connect…")
                    .font(AppTheme.Typography.body)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity)
            Button(role: .destructive) { cancelAndDismiss() } label: {
                Text("Cancel")
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, AppTheme.Spacing.sm)
            }
            .buttonStyle(.bordered)
            .tint(.secondary)
        }
    }

    private var connectedSection: some View {
        VStack(spacing: AppTheme.Spacing.md) {
            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 48))
                .foregroundStyle(AppTheme.Tint.success)
            Text("Connected")
                .font(AppTheme.Typography.headline)
            Text("Your signer app is linked. You can close this.")
                .font(AppTheme.Typography.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            Button {
                dismiss()
            } label: {
                Text("Done")
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, AppTheme.Spacing.sm)
            }
            .buttonStyle(.glassProminent)
        }
        .frame(maxWidth: .infinity)
        .padding(.top, AppTheme.Spacing.xl)
    }

    private func errorSection(_ message: String) -> some View {
        VStack(alignment: .leading, spacing: AppTheme.Spacing.sm) {
            if let qrImage { qrSection(qrImage) }
            HStack(alignment: .top, spacing: AppTheme.Spacing.xs) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(AppTheme.Tint.error)
                Text(message)
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(AppTheme.Tint.error)
            }
            Button {
                beginPairing()
            } label: {
                Label("Try again", systemImage: "arrow.clockwise")
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, AppTheme.Spacing.sm)
            }
            .buttonStyle(.glass)
        }
    }

    private var footnote: some View {
        Text("The QR code expires after 5 minutes. If it times out, come back to this screen to generate a new one.")
            .font(AppTheme.Typography.caption2)
            .foregroundStyle(.tertiary)
    }

    private var isPaired: Bool {
        if case .connected = identity.remoteSignerState { return true }
        return false
    }

    // MARK: - Actions

    private func beginPairing() {
        pairingTask?.cancel()
        nostrConnectURI = ""
        qrImage = nil
        detectSignerApps()
        let relay = pairingRelay
        pairingTask = Task {
            await identity.connectViaNostrConnect(relay: relay) { [self] uri in
                Task { @MainActor in
                    self.nostrConnectURI = uri
                    self.qrImage = makeQR(from: uri)
                }
            }
            if case .connected = identity.remoteSignerState {
                Haptics.success()
                try? await Task.sleep(for: .seconds(0.8))
                await MainActor.run { dismiss() }
            }
        }
    }

    private func cancelAndDismiss() {
        pairingTask?.cancel()
        Task { await identity.disconnectRemoteSigner() }
        dismiss()
    }

    private func openSignerApp(_ signer: KnownSigner) {
        guard !nostrConnectURI.isEmpty,
              let callback = "podcastr://nip46".addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed),
              let url = URL(string: "\(signer.urlScheme)://\(nostrConnectURI)&callback=\(callback)")
        else { return }
        UIApplication.shared.open(url)
    }

    private var pairingRelay: URL? {
        let configured = store.state.settings.nostrRelayURL.trimmed
        return configured.isEmpty ? nil : URL(string: configured)
    }

    private func detectSignerApps() {
        detectedSigners = KnownSigner.allCases.filter {
            UIApplication.shared.canOpenURL(URL(string: "\($0.urlScheme)://")!)
        }
    }

    // MARK: - QR generation

    private func makeQR(from string: String) -> UIImage? {
        let context = CIContext()
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(string.utf8)
        filter.correctionLevel = "M"
        guard let output = filter.outputImage else { return nil }
        let scaled = output.transformed(by: CGAffineTransform(scaleX: 10, y: 10))
        guard let cgImage = context.createCGImage(scaled, from: scaled.extent) else { return nil }
        return UIImage(cgImage: cgImage)
    }
}

// MARK: - KnownSigner

private enum KnownSigner: CaseIterable {
    case amber
    case primal

    var urlScheme: String {
        switch self {
        case .amber:  "nostrsigner"
        case .primal: "primal"
        }
    }

    var displayName: String {
        switch self {
        case .amber:  "Amber"
        case .primal: "Primal"
        }
    }

    var systemImage: String {
        switch self {
        case .amber:  "lock.shield"
        case .primal: "bolt.circle"
        }
    }
}
