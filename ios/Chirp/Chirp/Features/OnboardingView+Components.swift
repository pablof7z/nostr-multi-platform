import SwiftUI

extension OnboardingView {

    // MARK: — Animated background gradient

    var onboardingBackground: some View {
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
    }

    // MARK: — Decorative blur orbs (iOS 26 depth effect)

    var decorativeOrbs: some View {
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
    }

    // MARK: — Logo + brand

    var logoBrand: some View {
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
    }

    // MARK: — Import key card (mode == .importKey)

    var importKeyCard: some View {
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
}
