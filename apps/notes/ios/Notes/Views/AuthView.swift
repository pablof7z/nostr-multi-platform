import CoreImage.CIFilterBuiltins
import SwiftUI

/// Two-tab sign-in — nsec (local key) and bunker (NIP-46 remote signer).
/// Both feed through generic substrate seams; the spike adds no custom
/// auth code path.
struct AuthView: View {
    let bridge: NotesBridge
    @State private var nsecInput = ""
    @State private var bunkerURI: String?

    var body: some View {
        TabView {
            nsecTab.tabItem { Label("nsec", systemImage: "key.fill") }
            bunkerTab.tabItem { Label("Bunker", systemImage: "qrcode") }
        }
    }

    private var nsecTab: some View {
        VStack(spacing: 20) {
            Text("Sign in with nsec").font(.headline)
            SecureField("nsec1… or hex", text: $nsecInput)
                .textFieldStyle(.roundedBorder).autocorrectionDisabled()
                .textInputAutocapitalization(.never)
            Button("Sign in") {
                bridge.signInNsec(nsecInput.trimmingCharacters(in: .whitespaces))
            }
            .buttonStyle(.borderedProminent).disabled(nsecInput.isEmpty)
        }
        .padding()
    }

    private var bunkerTab: some View {
        VStack(spacing: 16) {
            Text("Scan with your signer app").font(.headline)
            if let uri = bunkerURI, let img = qrImage(from: uri) {
                Image(uiImage: img).interpolation(.none).resizable()
                    .frame(width: 240, height: 240)
                Text(uri).font(.caption2).lineLimit(2).truncationMode(.middle)
                    .padding(.horizontal)
                Button("I approved it") { bridge.isSignedIn = true }.buttonStyle(.bordered)
            } else {
                Button("Generate connect URI") { bunkerURI = bridge.generateBunkerURI() }
                    .buttonStyle(.borderedProminent)
            }
        }
        .padding()
    }

    private func qrImage(from string: String) -> UIImage? {
        let filter = CIFilter.qrCodeGenerator(); filter.message = Data(string.utf8)
        guard let out = filter.outputImage?.transformed(by: .init(scaleX: 10, y: 10)),
              let cg = CIContext().createCGImage(out, from: out.extent) else { return nil }
        return UIImage(cgImage: cg)
    }
}
