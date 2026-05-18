import SwiftUI

// MARK: - Layout constants

private enum Layout {
    static let avatarSize: CGFloat = 30
    static let avatarIconSize: CGFloat = 14
    static let bubbleCornerRadius: CGFloat = 18
    static let bubblePaddingH: CGFloat = 14
    static let bubblePaddingV: CGFloat = 10
    /// Corner radius for tool-batch and suggestion chip glass pills.
    static let pillCornerRadius: CGFloat = 14
    /// Horizontal padding inside tool-batch pill.
    static let batchPaddingH: CGFloat = 12
    /// Vertical padding inside tool-batch pill.
    static let batchPaddingV: CGFloat = 8
    /// Icon size (point size) for the tool-batch wand icon.
    static let batchIconSize: CGFloat = 13
    /// Leading indent of the tool-batch row (aligns with assistant bubble text).
    static let batchLeadingInset: CGFloat = 44
    /// Trailing inset of the tool-batch row.
    static let batchTrailingInset: CGFloat = 24
    /// Minimum leading space before the user's chat bubble, keeping it right-aligned.
    static let userBubbleMinLeading: CGFloat = 40
    /// Dot size in the typing indicator.
    static let typingDotSize: CGFloat = 7
    /// Spacing between typing indicator dots.
    static let typingDotSpacing: CGFloat = 6
    /// Corner radius for the typing indicator glass pill.
    static let typingCornerRadius: CGFloat = 18
    /// Horizontal padding inside the typing indicator pill.
    static let typingPaddingH: CGFloat = 14
    /// Vertical padding inside the typing indicator pill.
    static let typingPaddingV: CGFloat = 12
    /// Sub-xs gap between summary and secondary lines inside a tool-batch pill.
    static let batchSubSpacing: CGFloat = 1
}

// MARK: - Avatar

struct AgentAvatarView: View {
    var body: some View {
        ZStack {
            Circle()
                .fill(AppTheme.Gradients.agentAccent)
                .frame(width: Layout.avatarSize, height: Layout.avatarSize)
            Image(systemName: "sparkles")
                .font(.system(size: Layout.avatarIconSize, weight: .semibold))
                .foregroundStyle(.white)
                .accessibilityHidden(true)
        }
        .accessibilityHidden(true)
        .appShadow(AppTheme.Shadow.subtle)
    }
}

// MARK: - Chat Bubble

/// Renders a single chat message in the agent conversation, adapting its
/// appearance based on the message role (user, assistant, tool batch, or error).
struct AgentChatBubble: View {
    let message: ChatMessage
    var onOpenBatch: (UUID) -> Void = { _ in }
    /// Optional summary of the first action in a tool-batch — shown as a
    /// one-line preview inside the chip before the user taps to see all actions.
    var batchFirstSummary: String? = nil
    /// Number of actions in the batch that the user has undone.
    /// Used to dim and annotate the chip when all or some actions are reversed.
    var batchUndoneCount: Int = 0
    /// Called when the user taps "Retry" on an error bubble.
    /// When `nil`, no Retry button is shown.
    var onRetry: (() -> Void)? = nil
    /// Called when the user requests regeneration of the last assistant response.
    /// When `nil`, no Regenerate action appears in the context menu.
    var onRegenerate: (() -> Void)? = nil

    // MARK: - Pluralization helpers

    private static func agentRanLabel(count: Int) -> String {
        count == 1 ? "Agent ran 1 action" : "Agent ran \(count) actions"
    }

    private static func actionsUndoneLabel(count: Int) -> String {
        count == 1 ? "Action undone" : "All \(count) actions undone"
    }

    private static func moreActionsLabel(extra: Int) -> String {
        "+\(extra) more action\(extra == 1 ? "" : "s")"
    }

    var body: some View {
        switch message.role {
        case .user:
            userBubble
        case .assistant:
            assistantBubble
        case .toolBatch(let batchID, let count):
            toolBatchRow(batchID: batchID, count: count)
        case .error:
            errorBubble
        case .skillActivated(_, let displayName):
            skillActivatedIndicator(displayName: displayName)
        }
    }

    private var userBubble: some View {
        HStack {
            Spacer(minLength: Layout.userBubbleMinLeading)
            VStack(alignment: .trailing, spacing: 3) {
                Text(message.text)
                    .font(AppTheme.Typography.body)
                    .foregroundStyle(.white)
                    .padding(.horizontal, Layout.bubblePaddingH)
                    .padding(.vertical, Layout.bubblePaddingV)
                    .background(AppTheme.Gradients.agentAccent, in: .rect(cornerRadius: Layout.bubbleCornerRadius))
                    .overlay(
                        RoundedRectangle(cornerRadius: Layout.bubbleCornerRadius, style: .continuous)
                            .strokeBorder(.white.opacity(0.18), lineWidth: 0.5)
                    )
                    .appShadow(AppTheme.Shadow.subtle)
                    .copyableTextMenu(message.text)
                Text(message.timestamp, style: .time)
                    .font(AppTheme.Typography.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
    }

    private var assistantBubble: some View {
        HStack(alignment: .top, spacing: AppTheme.Spacing.sm) {
            AgentAvatarView()
            VStack(alignment: .leading, spacing: 3) {
                MarkdownView(text: message.text)
                    .padding(.horizontal, Layout.bubblePaddingH)
                    .padding(.vertical, Layout.bubblePaddingV)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .glassEffect(.regular, in: .rect(cornerRadius: Layout.bubbleCornerRadius))
                    .contextMenu {
                        Button {
                            UIPasteboard.general.string = message.text
                            Haptics.selection()
                        } label: {
                            Label("Copy", systemImage: "doc.on.doc")
                        }
                        if let onRegenerate {
                            Divider()
                            Button {
                                Haptics.selection()
                                onRegenerate()
                            } label: {
                                Label("Regenerate Response", systemImage: "arrow.clockwise")
                            }
                        }
                    }
                Text(message.timestamp, style: .time)
                    .font(AppTheme.Typography.caption2)
                    .foregroundStyle(.tertiary)
                    .padding(.leading, Layout.bubblePaddingH)
            }
            Spacer(minLength: 0)
        }
    }

    private func toolBatchRow(batchID: UUID, count: Int) -> some View {
        let allUndone = batchUndoneCount > 0 && batchUndoneCount == count
        let someUndone = batchUndoneCount > 0 && batchUndoneCount < count

        return Button {
            Haptics.selection()
            onOpenBatch(batchID)
        } label: {
            HStack(spacing: AppTheme.Spacing.sm) {
                Image(systemName: allUndone ? "wand.and.stars.inverse" : "wand.and.stars")
                    .font(.system(size: Layout.batchIconSize, weight: .semibold))
                    .foregroundStyle(allUndone ? AnyShapeStyle(.tertiary) : AnyShapeStyle(AppTheme.Tint.agentSurface))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: Layout.batchSubSpacing) {
                    if let summary = batchFirstSummary {
                        Text(summary)
                            .font(AppTheme.Typography.caption)
                            .foregroundStyle(allUndone ? AnyShapeStyle(.tertiary) : AnyShapeStyle(.primary))
                            .strikethrough(allUndone, color: .secondary)
                            .lineLimit(1)
                        if allUndone {
                            Text(Self.actionsUndoneLabel(count: count))
                                .font(AppTheme.Typography.caption2)
                                .foregroundStyle(.tertiary)
                        } else if someUndone {
                            Text("\(batchUndoneCount) of \(count) undone")
                                .font(AppTheme.Typography.caption2)
                                .foregroundStyle(.secondary)
                        } else if count > 1 {
                            Text(Self.moreActionsLabel(extra: count - 1))
                                .font(AppTheme.Typography.caption2)
                                .foregroundStyle(.tertiary)
                        }
                    } else {
                        Text(Self.agentRanLabel(count: count))
                            .font(AppTheme.Typography.caption)
                            .foregroundStyle(allUndone ? AnyShapeStyle(.tertiary) : AnyShapeStyle(.primary))
                            .strikethrough(allUndone, color: .secondary)
                        if allUndone {
                            Text(Self.actionsUndoneLabel(count: count))
                                .font(AppTheme.Typography.caption2)
                                .foregroundStyle(.tertiary)
                        } else if someUndone {
                            Text("\(batchUndoneCount) of \(count) undone")
                                .font(AppTheme.Typography.caption2)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .font(AppTheme.Typography.caption2)
                    .foregroundStyle(.tertiary)
                    .accessibilityHidden(true)
            }
            .padding(.horizontal, Layout.batchPaddingH)
            .padding(.vertical, Layout.batchPaddingV)
            .glassEffect(
                .regular.tint(allUndone ? .secondary.opacity(0.05) : AppTheme.Tint.agentSurface.opacity(0.10)).interactive(),
                in: .rect(cornerRadius: Layout.pillCornerRadius)
            )
        }
        .buttonStyle(.plain)
        .padding(.leading, Layout.batchLeadingInset)
        .padding(.trailing, Layout.batchTrailingInset)
        .accessibilityLabel(accessibilityLabelForBatch(
            count: count,
            allUndone: allUndone,
            someUndone: someUndone
        ))
        // Hint describes the effect — VoiceOver's button trait already
        // narrates "double-tap to activate" so the previous "Tap to
        // review." suffix on the label was redundant gesture-narration.
        .accessibilityHint("Reviews the actions in this batch")
    }

    private func accessibilityLabelForBatch(count: Int, allUndone: Bool, someUndone: Bool) -> String {
        let baseLabel: String
        if let summary = batchFirstSummary {
            let more = count > 1 ? ", and \(Self.moreActionsLabel(extra: count - 1))" : ""
            baseLabel = "\(summary)\(more)"
        } else {
            baseLabel = Self.agentRanLabel(count: count)
        }
        let undoneLabel: String
        if allUndone {
            undoneLabel = Self.actionsUndoneLabel(count: count) + "."
        } else if someUndone {
            undoneLabel = "\(batchUndoneCount) of \(count) undone."
        } else {
            undoneLabel = ""
        }
        return [baseLabel + ".", undoneLabel]
            .filter { !$0.isEmpty }
            .joined(separator: " ")
    }

    private func skillActivatedIndicator(displayName: String) -> some View {
        HStack(spacing: 4) {
            Image(systemName: "bolt.fill")
                .font(.system(size: 10, weight: .semibold))
                .foregroundStyle(AppTheme.Tint.agentSurface)
                .accessibilityHidden(true)
            Text("\(displayName) enabled")
                .font(AppTheme.Typography.caption2)
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .glassEffect(.regular.tint(AppTheme.Tint.agentSurface.opacity(0.08)), in: .capsule)
        .frame(maxWidth: .infinity, alignment: .center)
        .accessibilityLabel("\(displayName) skill enabled")
    }

    private var errorBubble: some View {
        HStack(alignment: .top, spacing: AppTheme.Spacing.sm) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(AppTheme.Tint.warning)
                .padding(.top, AppTheme.Spacing.sm)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: AppTheme.Spacing.sm) {
                Text(message.text)
                    .font(AppTheme.Typography.callout)
                    .foregroundStyle(.primary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if let onRetry {
                    Button("Retry") {
                        Haptics.selection()
                        onRetry()
                    }
                    .buttonStyle(.glass)
                    .controlSize(.small)
                    .accessibilityLabel("Retry sending message")
                }
            }
            .padding(.horizontal, Layout.bubblePaddingH)
            .padding(.vertical, Layout.bubblePaddingV)
            .frame(maxWidth: .infinity, alignment: .leading)
            .glassEffect(.regular.tint(AppTheme.Tint.warning.opacity(0.12)), in: .rect(cornerRadius: Layout.bubbleCornerRadius))
            .copyableTextMenu(message.text)
            Spacer(minLength: 0)
        }
    }

}
