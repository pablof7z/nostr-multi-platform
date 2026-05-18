import SwiftUI

// OWNER: Phase-2 Agent C may polish visuals/animation. The two kernel
// dispatches (signInNsec / createAccount) are the critical path and must
// keep working — RootShell gates the whole app on `model.hasActiveAccount`.

struct OnboardingView: View {
    @EnvironmentObject private var model: KernelModel
    @State private var nsec = ""
    @State private var mode: Mode = .welcome
    enum Mode { case welcome, importKey }

    var body: some View {
        ZStack {
            LinearGradient(colors: [ChirpColor.accent.opacity(0.35),
                ChirpColor.bg], startPoint: .top, endPoint: .bottom)
                .ignoresSafeArea()
            VStack(spacing: ChirpSpace.xl) {
                Spacer()
                VStack(spacing: ChirpSpace.m) {
                    Image(systemName: "bird.fill")
                        .font(.system(size: 64)).foregroundStyle(.white)
                    Text("Chirp").font(.system(size: 44,
                        weight: .bold, design: .rounded)).foregroundStyle(.white)
                    Text("A polished Nostr client")
                        .font(ChirpFont.callout).foregroundStyle(.white.opacity(0.8))
                }
                Spacer()
                if mode == .importKey {
                    GlassCard {
                        VStack(alignment: .leading, spacing: ChirpSpace.m) {
                            ChirpSectionHeader(title: "Private key")
                            SecureField("nsec1…", text: $nsec)
                                .font(ChirpFont.mono)
                                .textInputAutocapitalization(.never)
                                .autocorrectionDisabled()
                            ChirpPrimaryButton(title: "Sign in",
                                systemImage: "key.fill") {
                                model.signInNsec(nsec.trimmingCharacters(
                                    in: .whitespacesAndNewlines))
                            }
                        }
                    }
                    .padding(.horizontal, ChirpSpace.l)
                }
                VStack(spacing: ChirpSpace.m) {
                    if mode == .welcome {
                        ChirpPrimaryButton(title: "I have a key",
                            systemImage: "key.fill") {
                            withAnimation(.smooth) { mode = .importKey }
                        }
                        Button("Create a new identity") {
                            model.createAccount()
                        }
                        .font(ChirpFont.headline).foregroundStyle(.white)
                    } else {
                        Button("Back") {
                            withAnimation(.smooth) { mode = .welcome }
                        }
                        .font(ChirpFont.callout).foregroundStyle(.white.opacity(0.8))
                    }
                }
                .padding(.horizontal, ChirpSpace.l)
                Spacer().frame(height: ChirpSpace.xxl)
            }
        }
    }
}
