import SwiftUI

// MARK: - OnboardingView
//
// T-podcast-gap-002: Verbatim Podcastr OnboardingView requires UserIdentityStore
// and settings. Stub shows a minimal welcome that immediately marks onboarding
// complete so the main UI renders.
//
// Podcastr source:
// /Users/pablofernandez/Work/podcast/App/Sources/Features/Onboarding/OnboardingView.swift

struct OnboardingView: View {
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: AppTheme.Spacing.lg) {
            Spacer()
            Image(systemName: "waveform.circle.fill")
                .font(.system(size: 80))
                .foregroundStyle(Color.accentColor)
            Text("Podcastr")
                .font(AppTheme.Typography.largeTitle)
            Text("Your kernel-backed podcast app")
                .font(AppTheme.Typography.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            Spacer()
            Button("Get Started") {
                // T-podcast-gap-002: Set hasCompletedOnboarding via kernel settings
                // For now write directly to UserDefaults so the gate lifts.
                UserDefaults.standard.set(true, forKey: "hasCompletedOnboarding")
                dismiss()
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .padding(.horizontal, AppTheme.Spacing.xl)
            .padding(.bottom, AppTheme.Spacing.xl)
        }
        .padding(AppTheme.Spacing.lg)
    }
}
