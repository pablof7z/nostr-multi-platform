import SwiftUI

// Onboarding flow — three screens:
//   1. Welcome    (logo + two primary actions)
//   2. Create     (display name + create)
//   3. SignIn     (nsec / NIP-46 / bunker://)
//
// Screen components live in OnboardingView+Components.swift
// NIP-46 helpers live in OnboardingView+NIP46.swift

struct OnboardingView: View {
    @EnvironmentObject var model: KernelModel

    // -- Navigation --
    @State var mode: Mode = .welcome

    // -- Animation --
    @State var appeared = false

    // -- Create --
    @State var displayName = ""
    @FocusState var nameFieldFocused: Bool

    // -- Sign-in: nsec --
    @State var nsec = ""
    @FocusState var nsecFieldFocused: Bool

    // -- Sign-in: NIP-46 --
    @State var bunkerUri = ""
    @State var detectedSigner: DetectedSigner? = nil
    @State var nostrConnectURL: String? = nil
    @State var qrCodeImage: UIImage? = nil
    @State var showQR = false
    @State var nip46Tab: NIP46Tab = .qr

    enum Mode { case welcome, create, signIn }
    enum DetectedSigner: String {
        case nostrSigner = "Nostr Signer"
        case primal = "Primal"
        case other = "Signer"
    }
    enum NIP46Tab { case qr, uri }

    // MARK: — Body

    var body: some View {
        ZStack {
            Color(.systemBackground).ignoresSafeArea()

            switch mode {
            case .welcome:  welcomeScreen
            case .create:   createScreen
            case .signIn:   signInScreen
            }
        }
        .onAppear {
            withAnimation(.easeOut(duration: 0.35).delay(0.05)) {
                appeared = true
            }
            detectSignerApps()
            Task {
                if let uri = model.nostrConnectURI() {
                    nostrConnectURL = uri
                    qrCodeImage = generateQRCode(from: uri)
                }
            }
        }
    }
}
