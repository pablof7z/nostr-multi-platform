import SwiftUI
import CoreImage

extension OnboardingView {

    // MARK: — Remote signer (NIP-46) card

    var nip46SignerCard: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            Text("Remote signer (NIP-46)")
                .font(.caption)

            // QR code toggle
            Button {
                withAnimation(.smooth) { showQR.toggle() }
            } label: {
                HStack {
                    Image(systemName: "qrcode")
                        .foregroundStyle(Color.accentColor)
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
                            .background(Color(.systemBackground))
                            .clipShape(RoundedRectangle(cornerRadius: 12))
                            .overlay(
                                RoundedRectangle(cornerRadius: 12)
                                    .stroke(Color(.separator), lineWidth: 1)
                            )
                    } else {
                        RoundedRectangle(cornerRadius: 12)
                            .fill(Color(.secondarySystemBackground))
                            .frame(width: 200, height: 200)
                            .overlay { ProgressView().tint(Color.accentColor) }
                    }
                    Text("Scan with any NIP-46 signer app")
                        .font(ChirpFont.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity)
                .transition(.opacity.combined(with: .move(edge: .top)))
            }

            // Open in signer app — only when a compatible app is installed
            if let signer = detectedSigner, let url = nostrConnectURL {
                Button {
                    openSignerApp(url)
                } label: {
                    Label("Open in \(signer.rawValue)", systemImage: "arrow.up.forward.app")
                        .font(ChirpFont.callout)
                }
                .buttonStyle(.plain)
            }

            Divider()

            // Paste bunker:// URI — always visible
            VStack(alignment: .leading, spacing: ChirpSpace.s) {
                Text("Or paste a bunker:// URI")
                    .font(ChirpFont.caption)
                    .foregroundStyle(.secondary)

                TextField("bunker://…", text: $bunkerUri)
                    .font(ChirpFont.mono)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            }

            // Handshake progress
            if let handshake = model.bunkerHandshake, handshake.stage != "idle" {
                HStack(spacing: ChirpSpace.s) {
                    if handshake.stage == "connecting" || handshake.stage == "awaiting_pubkey" {
                        ProgressView().tint(Color.accentColor).scaleEffect(0.8)
                    } else if handshake.stage == "ready" {
                        Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                    } else if handshake.stage == "failed" {
                        Image(systemName: "xmark.circle.fill").foregroundStyle(.red)
                    }
                    Text(handshake.message ?? handshake.stage)
                        .font(ChirpFont.caption)
                        .foregroundStyle(.secondary)
                }
            }

            HStack(spacing: ChirpSpace.s) {
                Button {
                    model.signInBunker(bunkerUri.trimmingCharacters(in: .whitespacesAndNewlines))
                } label: {
                    Label("Connect", systemImage: "arrow.right.circle.fill")
                        .font(ChirpFont.headline)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 14)
                }
                .buttonStyle(.borderedProminent)
                .disabled(bunkerUri.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

                if let stage = model.bunkerHandshake?.stage,
                   stage == "connecting" || stage == "awaiting_pubkey" {
                    Button("Cancel") { model.cancelBunkerHandshake() }
                        .font(ChirpFont.callout)
                }
            }
        }
        .padding(.horizontal, ChirpSpace.l)
    }

    // MARK: — Helpers

    func detectSignerApps() {
        let schemes: [(String, DetectedSigner)] = [
            ("nostrsigner://", .nostrSigner),
            ("primal://", .primal),
        ]
        for (scheme, signer) in schemes {
            if let url = URL(string: scheme), UIApplication.shared.canOpenURL(url) {
                detectedSigner = signer
                return
            }
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

    func openSignerApp(_ connectURL: String) {
        var url = connectURL
        if let encoded = "chirp://nip46".addingPercentEncoding(withAllowedCharacters: .alphanumerics) {
            url += "&callback=\(encoded)"
        }
        if let u = URL(string: url) {
            UIApplication.shared.open(u)
        }
    }
}
