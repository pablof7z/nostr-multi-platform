import SwiftUI
import CoreImage

extension OnboardingView {

    // MARK: — Remote signer (NIP-46) GlassCard

    var nip46SignerCard: some View {
        GlassCard {
            VStack(alignment: .leading, spacing: ChirpSpace.m) {
                ChirpSectionHeader(title: "Remote signer (NIP-46)")

                // QR code toggle
                Button {
                    withAnimation(.smooth) { showQR.toggle() }
                } label: {
                    HStack {
                        Image(systemName: "qrcode")
                            .foregroundStyle(ChirpColor.accent)
                        Text(showQR ? "Hide QR code" : "Show nostrconnect:// QR")
                            .font(ChirpFont.callout)
                            .foregroundStyle(.white)
                        Spacer()
                        Image(systemName: showQR ? "chevron.up" : "chevron.down")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.white.opacity(0.6))
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
                                .background(.white)
                                .clipShape(RoundedRectangle(cornerRadius: 12))
                        } else {
                            RoundedRectangle(cornerRadius: 12)
                                .fill(.white.opacity(0.1))
                                .frame(width: 200, height: 200)
                                .overlay { ProgressView().tint(ChirpColor.accent) }
                        }
                        Text("Scan with any NIP-46 signer app")
                            .font(ChirpFont.caption)
                            .foregroundStyle(.white.opacity(0.65))
                    }
                    .frame(maxWidth: .infinity)
                    .transition(.opacity.combined(with: .move(edge: .top)))
                }

                // Open in signer app — only when a compatible app is installed
                if let signer = detectedSigner, let url = nostrConnectURL {
                    Button {
                        openSignerApp(url)
                    } label: {
                        HStack {
                            Image(systemName: "arrow.up.forward.app")
                                .foregroundStyle(ChirpColor.accent)
                            Text("Open in \(signer.rawValue)")
                                .font(ChirpFont.callout)
                                .foregroundStyle(.white)
                            Spacer()
                        }
                    }
                    .buttonStyle(.plain)
                }

                // Divider
                Divider().background(.white.opacity(0.2))

                // Paste bunker:// URI — always visible
                VStack(alignment: .leading, spacing: ChirpSpace.s) {
                    Text("Or paste a bunker:// URI")
                        .font(ChirpFont.caption)
                        .foregroundStyle(.white.opacity(0.65))

                    HStack(spacing: ChirpSpace.s) {
                        TextField("bunker://…", text: $bunkerUri)
                            .font(ChirpFont.mono)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .foregroundStyle(.white)

                        if let clip = UIPasteboard.general.string, clip.hasPrefix("bunker://") {
                            Button {
                                bunkerUri = clip
                            } label: {
                                HStack(spacing: 3) {
                                    Image(systemName: "doc.on.clipboard")
                                        .font(.system(size: 12, weight: .semibold))
                                    Text("Paste")
                                        .font(.system(.caption, design: .rounded).weight(.semibold))
                                }
                                .foregroundStyle(ChirpColor.accent)
                                .padding(.horizontal, ChirpSpace.s)
                                .padding(.vertical, 5)
                                .background(ChirpColor.accentSoft, in: Capsule())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }

                // Handshake progress
                if let handshake = model.bunkerHandshake, handshake.stage != "idle" {
                    HStack(spacing: ChirpSpace.s) {
                        if handshake.stage == "connecting" || handshake.stage == "awaiting_pubkey" {
                            ProgressView().tint(ChirpColor.accent).scaleEffect(0.8)
                        } else if handshake.stage == "ready" {
                            Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                        } else if handshake.stage == "failed" {
                            Image(systemName: "xmark.circle.fill").foregroundStyle(.red)
                        }
                        Text(handshake.message ?? handshake.stage)
                            .font(ChirpFont.caption)
                            .foregroundStyle(.white.opacity(0.8))
                    }
                }

                HStack(spacing: ChirpSpace.s) {
                    ChirpPrimaryButton(title: "Connect", systemImage: "arrow.right.circle.fill") {
                        model.signInBunker(bunkerUri.trimmingCharacters(in: .whitespacesAndNewlines))
                    }
                    .disabled(bunkerUri.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .opacity(bunkerUri.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? 0.5 : 1.0)

                    if let stage = model.bunkerHandshake?.stage,
                       stage == "connecting" || stage == "awaiting_pubkey" {
                        Button("Cancel") { model.cancelBunkerHandshake() }
                            .font(ChirpFont.callout)
                            .foregroundStyle(.white.opacity(0.7))
                    }
                }
            }
        }
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
