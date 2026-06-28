import SwiftUI

// Onboarding flow — welcome / create / sign-in screens. Pure view shell:
// no protocol state machine here (the NIP-46 typed onboarding model lives in
// Rust's `nip46_onboarding` projection); mode switching is pure navigation,
// which §4.6 allows in the view layer.

struct OnboardingView: View {
    @EnvironmentObject var model: KernelModel

    @State var mode: Mode = .welcome
    @State var appeared = false

    // -- Create --
    @State var displayName = ""
    @State var isCreatingAccount = false
    @FocusState var nameFieldFocused: Bool

    // -- Sign-in: nsec --
    @State var nsec = ""
    @FocusState var nsecFieldFocused: Bool

    // -- Sign-in: NIP-46 --
    @State var bunkerUri = ""
    /// Detected installed signer app — selected from Rust's
    /// `nip46Onboarding.signerApps` table by probing each scheme with
    /// `UIApplication.canOpenURL` (a §4.6 platform capability). Rust owns
    /// the table itself; Swift owns only the capability call.
    @State var detectedSignerApp: Nip46Onboarding.SignerApp? = nil
    @State var nostrConnectURL: String? = nil
    @State var qrCodeImage: UIImage? = nil
    @State var showQR = false

    enum Mode { case welcome, create, signIn }

    var body: some View {
        ZStack {
            ChirpBackdrop()

            switch mode {
            case .welcome:  welcomeScreen
            case .create:   createScreen
            case .signIn:   signInScreen
            }
        }
        .onAppear {
            appeared = true
            detectSignerApps()
            Task {
                if let uri = model.nostrConnectURI() {
                    nostrConnectURL = uri
                    qrCodeImage = generateQRCode(from: uri)
                }
            }
        }
        .onChange(of: model.nip46Onboarding?.signerApps) { _, _ in
            detectSignerApps()
        }
    }
}
