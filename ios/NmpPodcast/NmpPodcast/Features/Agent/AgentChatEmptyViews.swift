import SwiftUI

// MARK: - Layout

private enum Layout {
    static let bannerCloseIconSize: CGFloat = 11
    static let bannerCloseFrameSize: CGFloat = 22
    static let bannerIconSize: CGFloat = 14
    static let inputFieldPaddingH: CGFloat = 14
    static let inputFieldPaddingV: CGFloat = 10
    static let inputFieldCornerRadius: CGFloat = 22
    static let chipCornerRadius: CGFloat = 14
    static let bannerCornerRadius: CGFloat = 12
    static let welcomeIconSize: CGFloat = 44
    static let disconnectedIconSize: CGFloat = 40
}

// MARK: - Resume session banner

/// Banner shown at the top of the chat when the user is continuing a previous session.
struct AgentChatResumeBanner: View {
    @Binding var isDismissed: Bool

    var body: some View {
        HStack(spacing: AppTheme.Spacing.sm) {
            Image(systemName: "clock.arrow.circlepath")
                .font(.system(size: Layout.bannerIconSize, weight: .semibold))
                .foregroundStyle(AppTheme.Tint.agentSurface)
                .accessibilityHidden(true)
            Text("Continuing from your previous session")
                .font(AppTheme.Typography.caption)
                .foregroundStyle(.primary)
            Spacer(minLength: 0)
            Button {
                withAnimation(AppTheme.Animation.spring) { isDismissed = true }
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: Layout.bannerCloseIconSize, weight: .bold))
                    .foregroundStyle(.secondary)
                    .frame(
                        width: Layout.bannerCloseFrameSize,
                        height: Layout.bannerCloseFrameSize
                    )
            }
            .buttonStyle(.pressable)
            .accessibilityLabel("Dismiss banner")
        }
        .padding(.horizontal, Layout.inputFieldPaddingH)
        .padding(.vertical, Layout.inputFieldPaddingV)
        .glassEffect(
            .regular.tint(AppTheme.Tint.agentSurface.opacity(0.10)),
            in: .rect(cornerRadius: Layout.bannerCornerRadius)
        )
        .padding(.horizontal, AppTheme.Spacing.md)
        .padding(.top, AppTheme.Spacing.sm)
    }
}

// MARK: - Welcome state

/// Empty-state view when the agent is connected but no messages have been sent yet.
struct AgentChatWelcomeView: View {
    @Binding var draft: String
    var inputFocused: FocusState<Bool>.Binding

    @Environment(AppStateStore.self) private var store
    @State private var visibleSuggestions: [String] = []

    /// Suggestions for users who have at least one in-progress episode — the
    /// agent can pick up the listening thread directly via `list_in_progress`.
    private static let resumeSuggestions: [String] = [
        "What was I listening to?",
        "Where did I leave off?",
        "Summarize the episode I'm in the middle of",
    ]

    /// Suggestions for users with subscriptions but nothing in progress —
    /// agent can survey the queue via `list_recent_unplayed` and `list_subscriptions`.
    private static let subscribedSuggestions: [String] = [
        "What's new in my library?",
        "Catch me up on this week's episodes",
        "What should I listen to next?",
        "Pick a 30-minute episode for me",
    ]

    /// Suggestions for first-run users with no subscriptions — focused on
    /// onboarding the agent itself rather than library queries.
    private static let onboardingSuggestions: [String] = [
        "What can you help me with?",
        "Recommend a podcast on a topic I'd like",
        "Remember I prefer longer-form interviews",
        "How do I add a podcast?",
    ]

    var body: some View {
        VStack(spacing: AppTheme.Spacing.md) {
            Spacer()
            Image(systemName: "sparkles")
                .font(.system(size: Layout.welcomeIconSize, weight: .semibold))
                .foregroundStyle(AppTheme.Gradients.agentAccent)
                .symbolEffect(.pulse, options: .repeating)
            Text(timeOfDayGreeting)
                .font(AppTheme.Typography.title)
            Text("The agent can remember things, set reminders, take notes, and use tools you define.")
                .font(AppTheme.Typography.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, AppTheme.Spacing.lg)
            VStack(alignment: .leading, spacing: AppTheme.Spacing.sm) {
                ForEach(visibleSuggestions, id: \.self) { text in
                    suggestionChip(text)
                }
            }
            .padding(.top, AppTheme.Spacing.sm)
            Spacer()
        }
        .frame(maxWidth: .infinity)
        .padding(.horizontal, AppTheme.Spacing.lg)
        .onAppear { pickSuggestions() }
    }

    private func suggestionChip(_ text: String) -> some View {
        Button {
            Haptics.selection()
            draft = text
            inputFocused.wrappedValue = true
        } label: {
            HStack {
                Image(systemName: "wand.and.stars")
                    .font(AppTheme.Typography.caption)
                    .foregroundStyle(AppTheme.Tint.agentSurface)
                    .accessibilityHidden(true)
                Text(text)
                    .font(AppTheme.Typography.callout)
                    .foregroundStyle(.primary)
                    .multilineTextAlignment(.leading)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, Layout.inputFieldPaddingH)
            .padding(.vertical, Layout.inputFieldPaddingV)
            .frame(maxWidth: .infinity, alignment: .leading)
            .glassEffect(
                .regular.tint(AppTheme.Tint.agentSurface.opacity(0.08)).interactive(),
                in: .rect(cornerRadius: Layout.chipCornerRadius)
            )
        }
        .buttonStyle(.plain)
    }

    private func pickSuggestions() {
        let pool = suggestionPool()
        visibleSuggestions = Array(pool.shuffled().prefix(3))
    }

    /// Picks a context-aware suggestion pool based on the user's current
    /// library state. When suggestions land in front of the user, they should
    /// reflect the agent's actual capability *for them right now* — not
    /// generic memory prompts.
    private func suggestionPool() -> [String] {
        if !store.inProgressEpisodes.isEmpty {
            return Self.resumeSuggestions
        }
        if !store.state.subscriptions.isEmpty {
            return Self.subscribedSuggestions
        }
        return Self.onboardingSuggestions
    }

    private var timeOfDayGreeting: String {
        let hour = Calendar.current.component(.hour, from: Date())
        switch hour {
        case 5..<12:  return "Good morning"
        case 12..<17: return "Good afternoon"
        case 17..<21: return "Good evening"
        default:      return "Ask your agent anything"
        }
    }

}

// MARK: - Disconnected state

/// Empty-state view shown when the selected LLM provider is not yet connected.
struct AgentChatDisconnectedView: View {
    @Environment(AppStateStore.self) private var store

    var body: some View {
        VStack(spacing: AppTheme.Spacing.md) {
            Spacer()
            Image(systemName: "key.slash.fill")
                .font(.system(size: Layout.disconnectedIconSize))
                .foregroundStyle(AppTheme.Tint.warning)
            Text("Connect \(provider.displayName) to chat")
                .font(AppTheme.Typography.title)
                .multilineTextAlignment(.center)
            Text("The agent runs on the provider selected for the Agent role. Add that key in Settings to begin.")
                .font(AppTheme.Typography.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, AppTheme.Spacing.lg)
            NavigationLink {
                AIProvidersSettingsView()
            } label: {
                Label("Open Providers", systemImage: "key.viewfinder")
            }
            .buttonStyle(.glassProminent)
            .controlSize(.large)
            Spacer()
        }
        .frame(maxWidth: .infinity)
        .padding(.horizontal, AppTheme.Spacing.lg)
    }

    private var provider: LLMProvider {
        LLMModelReference(storedID: store.state.settings.agentInitialModel).provider
    }
}
