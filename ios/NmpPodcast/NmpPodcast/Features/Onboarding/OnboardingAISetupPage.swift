import SwiftUI

// MARK: - AI Setup

struct OnboardingAISetupPage: View {
    @Binding var apiKey: String
    var errorMessage: String?
    var isSaving: Bool
    var isConnectingBYOK: Bool
    var onConnectBYOK: () -> Void

    @State private var revealKey: Bool = false
    @State private var showManualEntry: Bool = false

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
        Image(systemName: "key.viewfinder")
            .font(.system(size: OnboardingLayout.pageIconSize, weight: .semibold))
            .foregroundStyle(.white)
            .symbolEffect(.pulse, options: .repeating)
            .padding(OnboardingLayout.pageIconPadding)
            .glassEffect(.regular, in: .circle)
            .overlay(Circle().strokeBorder(.white.opacity(OnboardingLayout.pageIconStroke), lineWidth: 1))
    }

    private var pageHeader: some View {
        VStack(spacing: AppTheme.Spacing.sm) {
            Text("Connect your providers")
                .font(AppTheme.Typography.largeTitle)
                .foregroundStyle(.white)

            Text("Connect BYOK once to approve OpenRouter, ElevenLabs, AssemblyAI, Ollama Cloud, and Perplexity. Skip and add them later in Settings.")
                .font(AppTheme.Typography.body)
                .foregroundStyle(.white.opacity(0.8))
                .multilineTextAlignment(.center)
                .padding(.horizontal, AppTheme.Spacing.md)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var actionArea: some View {
        VStack(spacing: AppTheme.Spacing.sm) {
            byokButton
            manualEntryToggle
            if showManualEntry { manualEntryField }
            if let errorMessage {
                Text(errorMessage)
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(AppTheme.Tint.errorOnDark)
                    .multilineTextAlignment(.center)
                    .transition(.opacity)
            }
        }
        .animation(AppTheme.Animation.springFast, value: showManualEntry)
        .animation(AppTheme.Animation.springFast, value: errorMessage)
    }

    private var byokButton: some View {
        Button {
            onConnectBYOK()
        } label: {
            HStack(spacing: AppTheme.Spacing.sm) {
                if isConnectingBYOK {
                    ProgressView()
                        .tint(.black)
                } else {
                    Image(systemName: "key.viewfinder")
                }
                Text(isConnectingBYOK ? "Connecting…" : "Connect BYOK Vault")
                    .font(AppTheme.Typography.headline)
            }
            .frame(maxWidth: .infinity, minHeight: OnboardingLayout.primaryButtonMinHeight)
            .padding(.vertical, OnboardingLayout.primaryButtonVerticalPadding)
        }
        .buttonStyle(.glassProminent)
        .controlSize(.large)
        .tint(.white)
        .foregroundStyle(.black)
        .disabled(isSaving)
    }

    private var manualEntryToggle: some View {
        Button {
            withAnimation(AppTheme.Animation.springFast) { showManualEntry.toggle() }
        } label: {
            Text(showManualEntry ? "Hide manual entry" : "Enter OpenRouter key manually")
                .font(AppTheme.Typography.callout)
                .foregroundStyle(.white.opacity(0.7))
        }
        .buttonStyle(.plain)
        .disabled(isSaving)
    }

    private var manualEntryField: some View {
        GlassEffectContainer {
            VStack(spacing: AppTheme.Spacing.sm) {
                HStack(spacing: AppTheme.Spacing.sm) {
                    Image(systemName: "key.fill")
                        .foregroundStyle(.white.opacity(0.7))
                    if revealKey {
                        TextField("sk-or-v1-…", text: $apiKey)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .foregroundStyle(.white)
                    } else {
                        SecureField("sk-or-v1-…", text: $apiKey)
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

                Text("Stored securely in Keychain.")
                    .font(AppTheme.Typography.caption2)
                    .foregroundStyle(.white.opacity(0.6))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, AppTheme.Spacing.sm)
            }
        }
        .transition(.opacity.combined(with: .move(edge: .top)))
    }
}
