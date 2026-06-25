import SwiftUI
import CoreImage

extension OnboardingView {

    // MARK: — Remote signer (NIP-46)
    //
    // Pure view: Rust's `nip46_onboarding` projection owns the typed
    // `stageKind` + pre-computed `isInFlight` / `isFailed` /
    // `isTerminalSuccess` / `canCancel` flags, plus the signer-app probe
    // table. No stage-string comparisons or URL string-mashing happens here.

    var nip46SignerSection: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            ChirpSectionHeader(title: "Remote signer (NIP-46)")

            // QR code toggle
            Button {
                showQR.toggle()
            } label: {
                HStack {
                    Image(systemName: "qrcode")
                        .foregroundStyle(ChirpColor.accent)
                    Text(showQR ? "Hide QR code" : "Show nostrconnect:// QR")
                        .font(ChirpFont.callout)
                    Spacer()
                    Image(systemName: showQR ? "chevron.up" : "chevron.down")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                }
            }
            .buttonStyle(.plain)

            if showQR {
                VStack(spacing: ChirpSpace.s) {
                    if let qr = qrCodeImage {
                        Image(uiImage: qr)
                            .resizable()
                            .interpolation(.none)
                            .scaledToFit()
                            .frame(maxWidth: 200)
                            .padding(16)
                            .background(.background, in: RoundedRectangle(cornerRadius: 12))
                    } else {
                        RoundedRectangle(cornerRadius: 12)
                            .fill(.quaternary)
                            .frame(width: 200, height: 200)
                            .overlay { ProgressView().tint(ChirpColor.accent) }
                    }
                    Text("Scan with any NIP-46 signer app")
                        .font(ChirpFont.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
            }

            if let signer = detectedSignerApp, let url = nostrConnectURL {
                Button {
                    openSignerApp(connectURL: url)
                } label: {
                    Label(
                        "Login with \(signer.displayLabel)",
                        systemImage: "arrow.up.forward.app"
                    )
                    .font(ChirpFont.callout)
                }
                .buttonStyle(.plain)
            }

            Divider()

            VStack(alignment: .leading, spacing: ChirpSpace.s) {
                Text("Or paste a bunker:// URI")
                    .font(ChirpFont.caption)
                    .foregroundStyle(.secondary)

                TextField("bunker://…", text: $bunkerUri)
                    .font(ChirpFont.mono)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            }

            if let onboarding = model.nip46Onboarding,
               let stage = onboarding.stageKind, stage != .idle {
                HStack(spacing: ChirpSpace.s) {
                    if onboarding.isInFlight {
                        ProgressView().tint(ChirpColor.accent).scaleEffect(0.8)
                    } else if onboarding.isTerminalSuccess {
                        Image(systemName: "checkmark.circle.fill").foregroundStyle(ChirpColor.success)
                    } else if onboarding.isFailed {
                        Image(systemName: "xmark.circle.fill").foregroundStyle(ChirpColor.danger)
                    }
                    Text(onboarding.localizedProgress ?? "")
                        .font(ChirpFont.caption)
                        .foregroundStyle(.secondary)
                }
            }

            HStack(spacing: ChirpSpace.s) {
                Button {
                    model.addSigner(bunkerUri: bunkerUri.trimmingCharacters(in: .whitespacesAndNewlines), makeActive: true)
                } label: {
                    Label("Connect", systemImage: "arrow.right.circle.fill")
                        .font(ChirpFont.headline)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 14)
                }
                .buttonStyle(.borderedProminent)
                .disabled(bunkerUri.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

                if model.nip46Onboarding?.canCancel == true {
                    Button("Cancel") { model.cancelBunkerHandshake() }
                        .font(ChirpFont.callout)
                }
            }
        }
        .padding(.horizontal, ChirpSpace.l)
        .padding(.vertical, ChirpSpace.l)
    }

    // MARK: — Helpers

    /// Iterate Rust's signer-app table and check `UIApplication.canOpenURL`
    /// for each scheme — Rust owns the protocol table; Swift owns only the
    /// platform-capability probe.
    func detectSignerApps() {
        guard let signerApps = model.nip46Onboarding?.signerApps else {
            detectedSignerApp = nil
            return
        }
        detectedSignerApp = signerApps.first { app in
            URL(string: app.scheme).map { UIApplication.shared.canOpenURL($0) } ?? false
        }
    }

    func generateQRCode(from string: String) -> UIImage? {
        guard let filter = CIFilter(name: "CIQRCodeGenerator") else { return nil }
        filter.setValue(Data(string.utf8), forKey: "inputMessage")
        filter.setValue("M", forKey: "inputCorrectionLevel")
        guard let output = filter.outputImage else { return nil }
        let scaled = output.transformed(by: CGAffineTransform(scaleX: 10, y: 10))
        let context = CIContext()
        guard let cgImage = context.createCGImage(scaled, from: scaled.extent) else { return nil }
        return UIImage(cgImage: cgImage)
    }

    /// Open the connect URL Rust generated verbatim — Rust already appended
    /// the `&callback=` query parameter so Swift performs no protocol-string
    /// composition (only the `UIApplication.open` platform capability).
    func openSignerApp(connectURL: String) {
        guard let url = URL(string: connectURL) else { return }
        UIApplication.shared.open(url)
    }
}
