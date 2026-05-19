import SwiftUI

// OWNER: Phase-2 Agent C may polish visuals/animation. The two kernel
// dispatches (signInNsec / createAccount) are the critical path and must
// keep working — RootShell gates the whole app on `model.hasActiveAccount`.
//
// Split: background + logo → OnboardingView+Components.swift
//        NIP-46 signer card + helpers → OnboardingView+NIP46.swift

struct OnboardingView: View {
    @EnvironmentObject var model: KernelModel
    @State var nsec = ""
    @State var bunkerUri = ""
    @State var detectedSigner: DetectedSigner? = nil
    @State var mode: Mode = .welcome
    @State var logoAppeared = false
    @State var contentAppeared = false
    @State var nostrConnectURL: String? = nil
    @State var qrCodeImage: UIImage? = nil
    @State var showQR = false

    enum Mode { case welcome, importKey }

    enum DetectedSigner: String {
        case nostrSigner = "Nostr Signer"
        case primal = "Primal"
        case other = "Signer"
    }

    var body: some View {
        VStack(spacing: ChirpSpace.xl) {
            Spacer()

            logoBrand

            Spacer()

            // Import key card
            if mode == .importKey {
                importKeyCard
            }

            // Action buttons
            VStack(spacing: ChirpSpace.m) {
                if mode == .welcome {
                    Button {
                        withAnimation(.smooth) { mode = .importKey }
                    } label: {
                        Label("I have a key", systemImage: "key.fill")
                            .font(ChirpFont.headline)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 14)
                    }
                    .buttonStyle(.borderedProminent)

                    nip46SignerCard

                    Button {
                        // CRITICAL DISPATCH — do not remove
                        model.createAccount()
                    } label: {
                        Text("Create a new identity")
                            .font(ChirpFont.headline)
                    }
                    .transition(.opacity)
                } else {
                    Button("Back") {
                        withAnimation(.smooth) { mode = .welcome }
                    }
                    .font(ChirpFont.callout)
                    .transition(.opacity)
                }
            }
            .padding(.horizontal, ChirpSpace.l)
            .opacity(contentAppeared ? 1 : 0)
            .offset(y: contentAppeared ? 0 : 16)

            Spacer().frame(height: ChirpSpace.xxl)
        }
        .background(Color(.systemBackground))
        .onAppear {
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
}
