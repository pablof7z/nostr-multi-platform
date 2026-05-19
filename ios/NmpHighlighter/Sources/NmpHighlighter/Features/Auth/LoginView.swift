import SwiftUI

/// Minimal login surface. Leans on iOS 26 Liquid Glass for chrome — almost no
/// custom styling, no heavy fills. Mirrors Olas's flow:
///   1. Detect known signer apps (Primal first).
///   2. If Primal present, surface a hero action: Scan / Paste / Show QR.
///   3. Always allow nsec paste + manual bunker URI paste as fallback.
struct LoginView: View {
    @Environment(HighlighterStore.self) private var store
    @Environment(\.openURL) private var openURL

    @State private var detectedSigner: KnownSigner?
    @State private var inputText: String = ""
    @State private var isWorking: Bool = false
    @State private var errorMessage: String?

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 32) {
                    header

                    if let detected = detectedSigner, detected == .primal {
                        primalHero
                    } else if let detected = detectedSigner {
                        genericSignerButton(detected)
                    }

                    manualEntry

                    if let errorMessage {
                        Text(errorMessage)
                            .font(.footnote)
                            .foregroundStyle(.red)
                    }
                }
                .padding(24)
            }
            .task {
                detectedSigner = KnownSigner.detect()
            }
            .navigationTitle("")
        }
    }

    // MARK: - Subviews

    private var header: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Highlighter")
                .font(.largeTitle.weight(.medium))
            Text("Sign in with your Nostr identity")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
    }

    private var primalHero: some View {
        VStack(spacing: 12) {
            Button {
                Task { await connectViaPrimalApp() }
            } label: {
                HStack(spacing: 12) {
                    Image(systemName: "bolt.fill")
                        .font(.title2)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Continue with Primal")
                            .font(.headline)
                        Text("Opens your Primal app to approve.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Image(systemName: "arrow.up.forward")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 14)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .buttonStyle(.glass)
            .disabled(isWorking)
        }
    }

    private func genericSignerButton(_ signer: KnownSigner) -> some View {
        Button {
            Task { await connectViaPrimalApp() }  // same flow, different scheme
        } label: {
            HStack {
                Text("Continue with \(signer.name)")
                Spacer()
                Image(systemName: "arrow.up.forward")
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 14)
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.glass)
        .disabled(isWorking)
    }

    private var manualEntry: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Or paste a key or bunker URI")
                .font(.caption)
                .foregroundStyle(.secondary)

            TextField("nsec1… or bunker://… or nostrconnect://…", text: $inputText, axis: .vertical)
                .lineLimit(1...3)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .padding(.horizontal, 14)
                .padding(.vertical, 12)
                .background(.thinMaterial, in: .rect(cornerRadius: 14))

            Button {
                Task { await submitManualInput() }
            } label: {
                Text(isWorking ? "Signing in…" : "Sign in")
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 10)
            }
            .buttonStyle(.glassProminent)
            .disabled(isWorking || inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

            NavigationLink {
                OnboardingCreateAccountView()
            } label: {
                Text("Create a new account")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding(.top, 4)
            }
        }
    }

    // MARK: - Actions

    private func submitManualInput() async {
        let trimmed = inputText.trimmingCharacters(in: .whitespacesAndNewlines)
        let normalized = trimmed.hasPrefix("nostr:") ? String(trimmed.dropFirst(6)) : trimmed
        guard !normalized.isEmpty else { return }

        isWorking = true
        errorMessage = nil
        defer { isWorking = false }

        do {
            if normalized.hasPrefix("nsec1") {
                let user = try await store.safeCore.loginNsec(normalized)
                AppSessionStore.shared.persistNsec(normalized)
                await store.completeLogin(user: user)
            } else if normalized.hasPrefix("bunker://") || normalized.hasPrefix("nostrconnect://") {
                let user = try await store.safeCore.pairBunker(normalized)
                AppSessionStore.shared.persistBunkerURI(normalized)
                await store.completeLogin(user: user)
            } else {
                errorMessage = "Enter an nsec1… or bunker:// URI."
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func connectViaPrimalApp() async {
        isWorking = true
        errorMessage = nil
        defer { isWorking = false }

        do {
            let options = NostrConnectOptions(
                name: "Highlighter",
                url: "https://highlighter.com",
                image: "https://highlighter.com/icon.png",
                perms: "sign_event:11,sign_event:1111,sign_event:9802,sign_event:16,nip04_encrypt,nip04_decrypt,nip44_encrypt,nip44_decrypt"
            )
            let uri = try await store.safeCore.startNostrConnect(options)

            // Attach a return-to-foreground callback; actual pairing happens
            // over the relay that the Rust core is already subscribed to.
            let callback = "highlighter://nip46"
            let encodedCallback = callback.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? callback
            let separator = uri.contains("?") ? "&" : "?"
            let urlWithCallback = "\(uri)\(separator)callback=\(encodedCallback)"

            if let url = URL(string: urlWithCallback) {
                openURL(url)
            }
            // `EventBridge` receives `.signerConnected(user)` once the remote
            // signer responds on the relay and `completeLogin` runs from there.
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
