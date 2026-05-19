import SwiftUI

// MARK: - Welcome

struct OnboardingWelcomePage: View {
    @State private var sparkleTrigger: Int = 0
    @State private var showChips = false

    var body: some View {
        VStack(spacing: AppTheme.Spacing.lg) {
            Spacer()
            sparkleMedallion
            VStack(spacing: AppTheme.Spacing.sm) {
                Text("Podcastr")
                    .font(AppTheme.Typography.largeTitle)
                    .foregroundStyle(.white)
                    .multilineTextAlignment(.center)

                Text("A podcast player with a memory")
                    .font(AppTheme.Typography.title)
                    .foregroundStyle(.white.opacity(0.85))
                    .multilineTextAlignment(.center)
            }

            Text("Subscribe to your shows; let an AI agent remember, summarize, and connect everything you listen to.")
                .font(AppTheme.Typography.body)
                .foregroundStyle(.white.opacity(0.75))
                .multilineTextAlignment(.center)
                .padding(.horizontal, AppTheme.Spacing.md)
                .fixedSize(horizontal: false, vertical: true)

            if showChips {
                HStack(spacing: AppTheme.Spacing.xs) {
                    featureChip("Subscribe", icon: "antenna.radiowaves.left.and.right")
                    featureChip("Listen", icon: "headphones")
                    featureChip("Remember", icon: "sparkles")
                }
                .transition(.move(edge: .bottom).combined(with: .opacity))
            }

            Spacer()
            Spacer()
        }
        .task {
            try? await Task.sleep(for: .seconds(0.5))
            withAnimation(AppTheme.Animation.spring) { showChips = true }
        }
    }

    private func featureChip(_ label: String, icon: String) -> some View {
        Label(label, systemImage: icon)
            .font(.system(size: 12, weight: .medium))
            .foregroundStyle(.white.opacity(0.9))
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(.white.opacity(0.15), in: Capsule())
    }

    private var sparkleMedallion: some View {
        ZStack {
            RoundedRectangle(cornerRadius: OnboardingLayout.medallionCornerRadius, style: .continuous)
                .fill(
                    LinearGradient(
                        colors: [
                            Color.white.opacity(0.35),
                            Color.white.opacity(0.05)
                        ],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )
                .frame(width: OnboardingLayout.medallionSize, height: OnboardingLayout.medallionSize)
                .glassEffect(.regular, in: .rect(cornerRadius: OnboardingLayout.medallionCornerRadius))
                .overlay(
                    RoundedRectangle(cornerRadius: OnboardingLayout.medallionCornerRadius, style: .continuous)
                        .strokeBorder(.white.opacity(0.35), lineWidth: 1)
                )
                .appShadow(AppTheme.Shadow.lifted)

            Image(systemName: "sparkles")
                .font(.system(size: OnboardingLayout.medallionIconSize, weight: .semibold))
                .foregroundStyle(AppTheme.Gradients.onboardingSparkle)
                .symbolEffect(.bounce, options: .repeat(3), value: sparkleTrigger)
                .appShadow(AppTheme.Shadow.onboardingIconGlow)
        }
        .onAppear {
            sparkleTrigger += 1
        }
    }
}
