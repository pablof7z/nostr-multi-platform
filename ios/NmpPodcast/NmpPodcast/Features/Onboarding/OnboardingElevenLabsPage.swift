import SwiftUI

// MARK: - OnboardingElevenLabsPage
//
// Optional onboarding step that mirrors `OnboardingAISetupPage` but for the
// ElevenLabs API key. Briefings + transcripts both depend on a configured
// ElevenLabs key, so we surface it during onboarding instead of deferring
// the discovery to the moment the user taps "Generate Briefing".
//
// Always skippable — the rest of the app degrades gracefully when the key
// is missing.

struct OnboardingElevenLabsPage: View {
    @Binding var apiKey: String
    var errorMessage: String?
    var isSaving: Bool

    @State private var revealKey: Bool = false

    var body: some View {
        VStack(spacing: AppTheme.Spacing.lg) {
            Spacer()
            pageIcon
            pageHeader
            actionArea
            Spacer()
        }
    }

    private var pageIcon: some View {
        Image(systemName: "waveform.circle.fill")
            .font(.system(size: OnboardingLayout.pageIconSize, weight: .semibold))
            .foregroundStyle(.white)
            .symbolEffect(.variableColor.iterative, options: .repeating)
            .padding(OnboardingLayout.pageIconPadding)
            .glassEffect(.regular, in: .circle)
            .overlay(Circle().strokeBorder(.white.opacity(OnboardingLayout.pageIconStroke), lineWidth: 1))
    }

    private var pageHeader: some View {
        VStack(spacing: AppTheme.Spacing.sm) {
            Text("Add an ElevenLabs key")
                .font(AppTheme.Typography.largeTitle)
                .foregroundStyle(.white)

            Text("Briefings and on-device transcripts use ElevenLabs. Optional — paste a key now, or skip and add it later in Settings.")
                .font(AppTheme.Typography.body)
                .foregroundStyle(.white.opacity(0.8))
                .multilineTextAlignment(.center)
                .padding(.horizontal, AppTheme.Spacing.md)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var actionArea: some View {
        VStack(spacing: AppTheme.Spacing.sm) {
            keyField
            if let errorMessage {
                Text(errorMessage)
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(AppTheme.Tint.errorOnDark)
                    .multilineTextAlignment(.center)
                    .transition(.opacity)
            }
        }
        .animation(AppTheme.Animation.springFast, value: errorMessage)
    }

    private var keyField: some View {
        GlassEffectContainer {
            VStack(spacing: AppTheme.Spacing.sm) {
                HStack(spacing: AppTheme.Spacing.sm) {
                    Image(systemName: "key.fill")
                        .foregroundStyle(.white.opacity(0.7))
                    if revealKey {
                        TextField("xi-api-…", text: $apiKey)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .foregroundStyle(.white)
                    } else {
                        SecureField("xi-api-…", text: $apiKey)
                            .foregroundStyle(.white)
                    }
                    Button {
                        revealKey.toggle()
                    } label: {
                        Image(systemName: revealKey ? "eye.slash.fill" : "eye.fill")
                            .foregroundStyle(.white.opacity(0.7))
                    }
                    .buttonStyle(.plain)
                    .disabled(isSaving)
                    .accessibilityLabel(revealKey ? "Hide API key" : "Show API key")
                }
                .padding(.horizontal, AppTheme.Spacing.md)
                .padding(.vertical, OnboardingLayout.fieldVerticalPadding)
                .glassEffect(.regular, in: .rect(cornerRadius: AppTheme.Corner.lg))
                .overlay(
                    RoundedRectangle(cornerRadius: AppTheme.Corner.lg, style: .continuous)
                        .strokeBorder(.white.opacity(0.25), lineWidth: 1)
                )

                Text("Stored securely in Keychain. Get a key at elevenlabs.io.")
                    .font(AppTheme.Typography.caption2)
                    .foregroundStyle(.white.opacity(0.6))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, AppTheme.Spacing.sm)
            }
        }
    }
}
