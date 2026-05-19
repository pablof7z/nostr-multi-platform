import SwiftUI

extension OnboardingView {

    // MARK: — Logo + brand

    var logoBrand: some View {
        VStack(spacing: ChirpSpace.m) {
            Image(systemName: "bird.fill")
                .font(.system(size: 44, weight: .medium))
                .foregroundStyle(Color.accentColor)
                .scaleEffect(logoAppeared ? 1 : 0.6)
                .opacity(logoAppeared ? 1 : 0)

            VStack(spacing: ChirpSpace.xs) {
                Text("Chirp")
                    .font(.largeTitle.weight(.bold))

                Text("A polished Nostr client")
                    .font(ChirpFont.callout)
                    .foregroundStyle(.secondary)
            }
            .opacity(contentAppeared ? 1 : 0)
            .offset(y: contentAppeared ? 0 : 12)
        }
    }

    // MARK: — Import key card (mode == .importKey)

    var importKeyCard: some View {
        VStack(alignment: .leading, spacing: ChirpSpace.m) {
            Text("Private key")
                .font(.caption)

            SecureField("nsec1…", text: $nsec)
                .font(ChirpFont.mono)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()

            Button {
                // CRITICAL DISPATCH — do not remove
                model.signInNsec(nsec.trimmingCharacters(in: .whitespacesAndNewlines))
            } label: {
                Label("Sign in", systemImage: "key.fill")
                    .font(ChirpFont.headline)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 14)
            }
            .buttonStyle(.borderedProminent)
            .disabled(nsec.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
        .padding(.horizontal, ChirpSpace.l)
        .transition(.move(edge: .bottom).combined(with: .opacity))
    }
}
