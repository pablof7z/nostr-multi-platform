import SwiftUI

/// Scrollable message transcript with a jump-to-bottom button.
struct AgentChatTranscriptView: View {

    private enum Layout {
        static let timeSeparatorThreshold: TimeInterval = 15 * 60
        static let jumpButtonSize: CGFloat = 30
        static let typingIndicatorID = "typing-indicator"
    }

    let session: AgentChatSession
    @Binding var scrolledMessageID: AnyHashable?
    let onBatchTap: (UUID) -> Void
    /// Called when the user taps "Retry" on an error bubble in the transcript.
    var onRetry: (() -> Void)? = nil
    /// Called when the user requests regeneration of the last assistant response.
    var onRegenerate: (() -> Void)? = nil

    @Environment(AppStateStore.self) private var store
    @State private var isAtBottom = true

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                messageList
            }
            .scrollDismissesKeyboard(.interactively)
            .scrollPosition(id: $scrolledMessageID, anchor: .bottom)
            .defaultScrollAnchor(.bottom)
            .tabBarMinimizeBehavior(.never)
            .overlay(alignment: .bottomTrailing) {
                if !isAtBottom {
                    jumpToBottomButton(proxy: proxy)
                }
            }
            .onChange(of: session.messages.count) { oldCount, newCount in
                guard newCount > oldCount, let last = session.messages.last else { return }
                guard isAtBottom else { return }
                withAnimation(AppTheme.Animation.spring) {
                    proxy.scrollTo(last.id, anchor: .bottom)
                }
            }
            .onChange(of: session.phase) {
                if case .sending = session.phase {
                    withAnimation(AppTheme.Animation.spring) {
                        proxy.scrollTo(Layout.typingIndicatorID, anchor: .bottom)
                    }
                }
            }
            .onChange(of: session.streamingContent) { _, content in
                guard let content, !content.isEmpty, isAtBottom else { return }
                proxy.scrollTo(Layout.typingIndicatorID, anchor: .bottom)
            }
            .onChange(of: session.currentToolName) { _, _ in
                guard isAtBottom else { return }
                proxy.scrollTo(Layout.typingIndicatorID, anchor: .bottom)
            }
        }
    }

    private func jumpToBottomButton(proxy: ScrollViewProxy) -> some View {
        Button {
            Haptics.selection()
            withAnimation(AppTheme.Animation.spring) {
                if let lastID = session.messages.last?.id {
                    proxy.scrollTo(lastID, anchor: .bottom)
                }
            }
        } label: {
            Image(systemName: "chevron.down.circle.fill")
                .font(.system(size: Layout.jumpButtonSize))
                .symbolRenderingMode(.hierarchical)
                .foregroundStyle(AppTheme.Gradients.agentAccent)
        }
        .buttonStyle(.pressable)
        .padding(AppTheme.Spacing.md)
        .transition(.scale.combined(with: .opacity))
        .accessibilityLabel("Jump to latest message")
    }

    private var messageList: some View {
        LazyVStack(alignment: .leading, spacing: AppTheme.Spacing.md) {
            ForEach(Array(session.messages.enumerated()), id: \.element.id) { index, msg in
                let prev = index > 0 ? session.messages[index - 1] : nil
                if shouldShowSeparator(before: msg, previous: prev) {
                    ChatTimeSeparator(date: msg.timestamp)
                        .transition(.opacity)
                }
                AgentChatBubble(
                    message: msg,
                    onOpenBatch: onBatchTap,
                    batchFirstSummary: batchSummary(for: msg),
                    batchUndoneCount: batchUndoneCount(for: msg),
                    onRetry: retryCallback(for: msg, isLast: index == session.messages.count - 1),
                    onRegenerate: regenerateCallback(for: msg, isLast: index == session.messages.count - 1)
                )
                .id(msg.id)
                .transition(.opacity.combined(with: .move(edge: .bottom)))
            }
            if case .sending = session.phase {
                if let partial = session.streamingContent, !partial.isEmpty {
                    AgentChatBubble(
                        message: ChatMessage(role: .assistant, text: partial),
                        onOpenBatch: { _ in }
                    )
                    .id(Layout.typingIndicatorID)
                    .transition(.opacity)
                } else {
                    AgentTypingIndicator(toolName: session.currentToolName)
                        .id(Layout.typingIndicatorID)
                        .transition(.opacity)
                }
            }
            Color.clear.frame(height: 1)
                .onScrollVisibilityChange(threshold: 0.01) { visible in
                    withAnimation(AppTheme.Animation.springFast) {
                        isAtBottom = visible
                    }
                }
        }
        .padding(.horizontal, AppTheme.Spacing.md)
        .padding(.top, AppTheme.Spacing.md)
        .padding(.bottom, AppTheme.Spacing.sm)
        .animation(AppTheme.Animation.spring, value: session.messages.count)
        .animation(AppTheme.Animation.spring, value: session.phase)
    }

    // MARK: - Retry callback

    /// Returns the retry callback for an error bubble, or `nil` if the bubble
    /// should not display a Retry button.
    ///
    /// The Retry button is only shown on the last message in the transcript when:
    /// - the message role is `.error`, and
    /// - the session has a `lastFailedMessage` (i.e. retry is possible), and
    /// - an `onRetry` handler was provided by the parent view.
    private func retryCallback(for msg: ChatMessage, isLast: Bool) -> (() -> Void)? {
        guard case .error = msg.role,
              isLast,
              session.lastFailedMessage != nil,
              let onRetry else { return nil }
        return onRetry
    }

    // MARK: - Regenerate callback

    /// Returns the regenerate callback for an assistant bubble, or `nil` if
    /// regeneration is not available for this bubble.
    ///
    /// Regenerate is only shown on the last assistant message in the transcript
    /// when `session.canRegenerate` is true (the qualifying user→assistant
    /// turn with no tool calls in between) and an `onRegenerate` handler was
    /// provided by the parent view.
    private func regenerateCallback(for msg: ChatMessage, isLast: Bool) -> (() -> Void)? {
        guard case .assistant = msg.role,
              isLast,
              session.canRegenerate,
              let onRegenerate else { return nil }
        return onRegenerate
    }

    // MARK: - Batch summary helpers

    /// Returns the summary of the first activity entry for a tool-batch message, if any.
    /// Used to show a one-line preview inside the tool-batch chip.
    private func batchSummary(for msg: ChatMessage) -> String? {
        guard case .toolBatch(let batchID, _) = msg.role else { return nil }
        // agentActivity(forBatch:) returns entries sorted newest-first; the
        // last element is the first action that was executed.
        return store.agentActivity(forBatch: batchID).last?.summary
    }

    /// Returns how many activity entries in this batch have been undone.
    /// Drives the undo-state indicator on the tool-batch chip.
    private func batchUndoneCount(for msg: ChatMessage) -> Int {
        guard case .toolBatch(let batchID, _) = msg.role else { return 0 }
        return store.agentActivity(forBatch: batchID).filter(\.undone).count
    }

    // MARK: - Timestamp separator logic

    /// Returns true when a time-gap separator should be shown before `msg`.
    /// Separators appear before the first conversational message in a session,
    /// and whenever 15+ minutes have elapsed since the previous message.
    /// Tool-batch and error rows (system events) never get separators.
    private func shouldShowSeparator(before msg: ChatMessage, previous prev: ChatMessage?) -> Bool {
        switch msg.role {
        case .user, .assistant: break
        case .toolBatch, .error, .skillActivated: return false
        }
        guard let prev else { return true }
        switch prev.role {
        case .user, .assistant:
            return msg.timestamp.timeIntervalSince(prev.timestamp) >= Layout.timeSeparatorThreshold
        case .toolBatch, .error, .skillActivated:
            return false
        }
    }

    // MARK: - ChatTimeSeparator

    /// Centered timestamp label shown between message groups separated by ≥ 15 minutes.
    /// Uses a human-friendly format: "Today 2:34 PM", "Yesterday 9:15 AM",
    /// "Mon 2:34 PM" (within the last week), or "Mar 5, 2:34 PM" (older).
    private struct ChatTimeSeparator: View {
    let date: Date

    var body: some View {
        Text(formattedDate)
            .font(AppTheme.Typography.caption2)
            .foregroundStyle(.tertiary)
            .frame(maxWidth: .infinity, alignment: .center)
            .padding(.vertical, AppTheme.Spacing.xs)
            .accessibilityLabel(accessibilityLabel)
    }

    private var formattedDate: String {
        let calendar = Calendar.current
        let now = Date()
        let timeStr = date.formatted(date: .omitted, time: .shortened)
        if calendar.isDateInToday(date) {
            return "Today \(timeStr)"
        } else if calendar.isDateInYesterday(date) {
            return "Yesterday \(timeStr)"
        } else if let days = calendar.dateComponents([.day], from: date, to: now).day, days < 7 {
            let dayName = date.formatted(.dateTime.weekday(.wide))
            return "\(dayName) \(timeStr)"
        } else {
            return date.formatted(.dateTime.month(.abbreviated).day().hour().minute())
        }
    }

    private var accessibilityLabel: String {
        date.formatted(date: .complete, time: .shortened)
    }
    }
}
