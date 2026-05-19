import SwiftUI

// MARK: - OnboardingView
//
// Hosts the full onboarding flow as a horizontally-paged `TabView`. The page
// list is driven by `OnboardingStep` (typed, not magic integers) so the
// primary-button label, skip-visibility, and per-step actions all read from
// a single source of truth.
//
// Per-step action handlers live in `OnboardingView+Handlers.swift` to keep
// this file under the 300-line soft limit. State is intentionally `internal`
// (not `private`) so the handler extension can mutate it without indirection.

struct OnboardingView: View {
    @Environment(AppStateStore.self) var store

    @State var step: OnboardingStep = .welcome

    // Provider setup state
    @State var apiKeyDraft: String = ""
    @State var apiKeyError: String?
    @State var apiKeySaving: Bool = false
    @State var isConnectingBYOK: Bool = false
    @State var byokConnect = BYOKConnectService()

    // Identity state
    @State var agentNameDraft: String = ""
    @State var profilePictureDraft: String = ""

    // Subscribe state
    /// Tracks whether the user has subscribed to at least one show during
    /// onboarding. Used to flip the primary button label from "Skip for Now"
    /// to "Continue".
    @State var hasSubscribedDuringOnboarding: Bool = false

    private enum Layout {
        static let chipHorizontalPadding: CGFloat = 14
        static let chipVerticalPadding: CGFloat = 8
        /// Height of the top bar (back / skip buttons row).
        static let topBarHeight: CGFloat = 60
    }

    var body: some View {
        ZStack {
            backgroundGradient.ignoresSafeArea()

            VStack(spacing: 0) {
                topBar
                pages
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                bottomBar
            }
        }
        .preferredColorScheme(.dark)
    }

    // MARK: - Pages

    private var pages: some View {
        TabView(selection: $step) {
            OnboardingWelcomePage()
                .tag(OnboardingStep.welcome)
                .padding(.horizontal, AppTheme.Spacing.lg)

            OnboardingAISetupPage(
                apiKey: $apiKeyDraft,
                errorMessage: apiKeyError,
                isSaving: apiKeySaving || isConnectingBYOK,
                isConnectingBYOK: isConnectingBYOK,
                onConnectBYOK: { Task { await handleBYOKConnect() } }
            )
            .tag(OnboardingStep.aiSetup)
            .padding(.horizontal, AppTheme.Spacing.lg)

            OnboardingIdentityPage(
                agentName: $agentNameDraft,
                profilePicture: $profilePictureDraft
            )
            .tag(OnboardingStep.identity)
            .padding(.horizontal, AppTheme.Spacing.lg)

            OnboardingSubscribePage { _ in
                // Don't auto-advance: the user just tapped a row and we
                // want them to *see* it flip to the green checkmark before
                // the page transitions away (otherwise the tap looks like
                // a no-op). Flipping `hasSubscribedDuringOnboarding`
                // re-labels the primary button to "Continue", which the
                // user then taps when they're ready to move on.
                hasSubscribedDuringOnboarding = true
            }
            .tag(OnboardingStep.subscribe)
            .padding(.horizontal, AppTheme.Spacing.lg)

            OnboardingReadyPage()
                .tag(OnboardingStep.ready)
                .padding(.horizontal, AppTheme.Spacing.lg)
        }
        .tabViewStyle(.page(indexDisplayMode: .always))
        .indexViewStyle(.page(backgroundDisplayMode: .always))
        .animation(AppTheme.Animation.spring, value: step)
    }

    // MARK: - Top bar

    private var topBar: some View {
        HStack {
            if step != .welcome { backButton }
            Spacer()
            if step.allowsSkip { skipButton }
        }
        .padding(.horizontal, AppTheme.Spacing.md)
        .padding(.top, AppTheme.Spacing.md)
        .frame(height: Layout.topBarHeight)
    }

    private var backButton: some View {
        Button {
            Haptics.selection()
            withAnimation(AppTheme.Animation.spring) {
                if let prev = step.previous { step = prev }
            }
        } label: {
            Label("Back", systemImage: "chevron.left")
                .font(AppTheme.Typography.callout.weight(.semibold))
                .foregroundStyle(.white)
                .padding(.horizontal, Layout.chipHorizontalPadding)
                .padding(.vertical, Layout.chipVerticalPadding)
                .glassEffect(.regular.interactive(), in: .capsule)
        }
        .buttonStyle(.plain)
    }

    private var skipButton: some View {
        Button {
            Haptics.selection()
            advance()
        } label: {
            Text("Skip")
                .font(AppTheme.Typography.callout.weight(.semibold))
                .foregroundStyle(.white.opacity(0.85))
                .padding(.horizontal, Layout.chipHorizontalPadding)
                .padding(.vertical, Layout.chipVerticalPadding)
                .glassEffect(.regular.interactive(), in: .capsule)
        }
        .buttonStyle(.plain)
    }

    // MARK: - Bottom bar

    private var bottomBar: some View {
        VStack(spacing: AppTheme.Spacing.sm) {
            Button {
                Haptics.medium()
                primaryAction()
            } label: {
                HStack(spacing: AppTheme.Spacing.sm) {
                    Text(primaryButtonTitle)
                        .font(AppTheme.Typography.headline)
                    Image(systemName: step.isLast ? "sparkles" : "arrow.right")
                        .font(AppTheme.Typography.headline)
                }
                .frame(maxWidth: .infinity, minHeight: OnboardingLayout.primaryButtonMinHeight)
                .padding(.vertical, OnboardingLayout.primaryButtonVerticalPadding)
            }
            .buttonStyle(.glassProminent)
            .controlSize(.large)
            .tint(.white)
            .foregroundStyle(.black)
            .disabled(isPrimaryButtonDisabled)
        }
        .padding(.horizontal, AppTheme.Spacing.lg)
        .padding(.bottom, AppTheme.Spacing.lg)
        .padding(.top, AppTheme.Spacing.sm)
    }

    // MARK: - Primary button labelling

    private var primaryButtonTitle: String {
        switch step {
        case .welcome:    return "Get Started"
        case .aiSetup:    return apiKeyDraft.isBlank ? "Skip for Now" : "Save Key"
        case .identity:   return agentNameDraft.isBlank ? "Skip for Now" : "Save"
        case .subscribe:  return hasSubscribedDuringOnboarding ? "Continue" : "Skip for Now"
        case .ready:      return "Enter App"
        }
    }

    private var isPrimaryButtonDisabled: Bool {
        apiKeySaving || isConnectingBYOK
    }

    // MARK: - Routing

    private func primaryAction() {
        switch step {
        case .aiSetup:
            handleAISetupContinue()
        case .identity:
            handleIdentityContinue()
        case .ready:
            finishOnboarding()
        case .welcome, .subscribe:
            advance()
        }
    }

    func advance() {
        withAnimation(AppTheme.Animation.spring) {
            if let next = step.next {
                step = next
            } else {
                finishOnboarding()
            }
        }
    }

    // MARK: - Background

    private var backgroundGradient: LinearGradient {
        AppTheme.Gradients.onboardingNebula
    }
}
