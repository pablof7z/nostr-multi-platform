import SwiftUI
import CoreImage

// OWNER: Phase-2 Agent C may polish visuals/animation. The two kernel
// dispatches (signInNsec / createAccount) are the critical path and must
// keep working — RootShell gates the whole app on `model.hasActiveAccount`.

struct OnboardingView: View {
    @EnvironmentObject private var model: KernelModel
    @State private var nsec = ""
    @State private var bunkerUri = ""
    @State private var detectedSigner: DetectedSigner? = nil
    @State private var mode: Mode = .welcome
    @State private var animateGradient = false
    @State private var logoAppeared = false
    @State private var contentAppeared = false
    @State private var nostrConnectURL: String? = nil
    @State private var qrCodeImage: UIImage? = nil
    @State private var showQR = false

    enum Mode { case welcome, importKey }

    enum DetectedSigner: String {
        case nostrSigner = "Nostr Signer"
        case primal = "Primal"
        case other = "Signer"
    }

    var body: some View {
        ZStack {
            // Animated gradient background
            LinearGradient(
                colors: [
                    ChirpColor.accent.opacity(animateGradient ? 0.55 : 0.35),
                    Color(red: 0.20, green: 0.10, blue: 0.50).opacity(animateGradient ? 0.8 : 0.6),
                    ChirpColor.bg,
                ],
                startPoint: animateGradient ? .topLeading : .top,
                endPoint: animateGradient ? .bottomTrailing : .bottom
            )
            .ignoresSafeArea()
            .animation(.easeInOut(duration: 6).repeatForever(autoreverses: true), value: animateGradient)

            // Decorative blur orbs (iOS 26 depth effect)
            GeometryReader { geo in
                Circle()
                    .fill(ChirpColor.accent.opacity(0.25))
                    .frame(width: geo.size.width * 0.7)
                    .blur(radius: 80)
                    .offset(x: -geo.size.width * 0.1, y: geo.size.height * 0.05)
                    .scaleEffect(animateGradient ? 1.1 : 0.9)
                    .animation(.easeInOut(duration: 8).repeatForever(autoreverses: true), value: animateGradient)

                Circle()
                    .fill(Color(red: 0.30, green: 0.10, blue: 0.90).opacity(0.20))
                    .frame(width: geo.size.width * 0.5)
                    .blur(radius: 60)
                    .offset(x: geo.size.width * 0.5, y: geo.size.height * 0.6)
                    .scaleEffect(animateGradient ? 0.9 : 1.1)
                    .animation(.easeInOut(duration: 7).repeatForever(autoreverses: true).delay(1), value: animateGradient)
            }
            .ignoresSafeArea()

            // Main content
            VStack(spacing: ChirpSpace.xl) {
                Spacer()

                // Logo + brand
                VStack(spacing: ChirpSpace.m) {
                    ZStack {
                        Circle()
                            .fill(.ultraThinMaterial)
                            .frame(width: 96, height: 96)
                            .overlay(
                                Circle().strokeBorder(Color.white.opacity(0.25))
                            )
                        Image(systemName: "bird.fill")
                            .font(.system(size: 44, weight: .medium))
                            .foregroundStyle(.white)
                    }
                    .scaleEffect(logoAppeared ? 1 : 0.6)
                    .opacity(logoAppeared ? 1 : 0)

                    VStack(spacing: ChirpSpace.xs) {
                        Text("Chirp")
                            .font(.system(size: 48, weight: .bold, design: .rounded))
                            .foregroundStyle(.white)

                        Text("A polished Nostr client")
                            .font(ChirpFont.callout)
                            .foregroundStyle(.white.opacity(0.75))
                    }
                    .opacity(contentAppeared ? 1 : 0)
                    .offset(y: contentAppeared ? 0 : 12)
                }

                Spacer()

                // Import key card
                if mode == .importKey {
                    GlassCard {
                        VStack(alignment: .leading, spacing: ChirpSpace.m) {
                            ChirpSectionHeader(title: "Private key")

                            // nsec input with clipboard paste affordance
                            HStack(spacing: ChirpSpace.s) {
                                SecureField("nsec1…", text: $nsec)
                                    .font(ChirpFont.mono)
                                    .textInputAutocapitalization(.never)
                                    .autocorrectionDisabled()

                                // Paste from clipboard button
                                if let clip = UIPasteboard.general.string,
                                   clip.hasPrefix("nsec") {
                                    Button {
                                        nsec = clip
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
                                    .transition(.scale.combined(with: .opacity))
                                }
                            }

                            ChirpPrimaryButton(title: "Sign in", systemImage: "key.fill") {
                                // CRITICAL DISPATCH — do not remove
                                model.signInNsec(nsec.trimmingCharacters(in: .whitespacesAndNewlines))
                            }
                            .disabled(nsec.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                            .opacity(nsec.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? 0.5 : 1.0)
                        }
                    }
                    .padding(.horizontal, ChirpSpace.l)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
                }

                // Action buttons
                VStack(spacing: ChirpSpace.m) {
                    if mode == .welcome {
                        ChirpPrimaryButton(title: "I have a key", systemImage: "key.fill") {
                            withAnimation(.smooth) { mode = .importKey }
                        }

                        // Remote signer section — always visible
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

                        Button("Create a new identity") {
                            // CRITICAL DISPATCH — do not remove
                            model.createAccount()
                        }
                        .font(ChirpFont.headline)
                        .foregroundStyle(.white)
                        .transition(.opacity)
                    } else {
                        Button("Back") {
                            withAnimation(.smooth) { mode = .welcome }
                        }
                        .font(ChirpFont.callout)
                        .foregroundStyle(.white.opacity(0.8))
                        .transition(.opacity)
                    }
                }
                .padding(.horizontal, ChirpSpace.l)
                .opacity(contentAppeared ? 1 : 0)
                .offset(y: contentAppeared ? 0 : 16)

                Spacer().frame(height: ChirpSpace.xxl)
            }
        }
        .onAppear {
            animateGradient = true
            withAnimation(.spring(response: 0.7, dampingFraction: 0.65).delay(0.15)) {
                logoAppeared = true
            }
            withAnimation(.smooth(duration: 0.5).delay(0.4)) {
                contentAppeared = true
            }
            detectSignerApps()
        }
        .task {
            detectSignerApps()
            if let uri = model.nostrConnectURI() {
                nostrConnectURL = uri
                qrCodeImage = generateQRCode(from: uri)
            }
        }
    }

    private func detectSignerApps() {
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

    private func generateQRCode(from string: String) -> UIImage? {
        guard let filter = CIFilter(name: "CIQRCodeGenerator") else { return nil }
        filter.setValue(Data(string.utf8), forKey: "inputMessage")
        filter.setValue("M", forKey: "inputCorrectionLevel")
        guard let output = filter.outputImage else { return nil }
        let scaled = output.transformed(by: CGAffineTransform(scaleX: 10, y: 10))
        let context = CIContext()
        guard let cgImage = context.createCGImage(scaled, from: scaled.extent) else { return nil }
        return UIImage(cgImage: cgImage)
    }

    private func openSignerApp(_ connectURL: String) {
        var url = connectURL
        if let encoded = "chirp://nip46".addingPercentEncoding(withAllowedCharacters: .alphanumerics) {
            url += "&callback=\(encoded)"
        }
        if let u = URL(string: url) {
            UIApplication.shared.open(u)
        }
    }
}
