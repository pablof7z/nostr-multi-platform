import SwiftUI

// MARK: - Typing Indicator

/// Animated three-dot indicator shown while the agent is generating a response.
/// When the agent is executing a tool, `toolName` is non-nil and a status label
/// replaces the bouncing dots so the user can see what the agent is doing.
struct AgentTypingIndicator: View {

    private enum Layout {
        static let batchIconSize: CGFloat = 13
        static let typingDotSize: CGFloat = 7
        static let typingDotSpacing: CGFloat = 6
        static let typingCornerRadius: CGFloat = AppTheme.Corner.bubble
        static let typingPaddingH: CGFloat = 14
        static let typingPaddingV: CGFloat = 12
    }

    /// The raw tool name currently being dispatched (e.g. "create_item"),
    /// or `nil` when the agent is streaming text / waiting for an LLM reply.
    var toolName: String? = nil

    @State private var phase: Int = 0

    var body: some View {
        HStack(alignment: .top, spacing: AppTheme.Spacing.sm) {
            AgentAvatarView()
                .symbolEffect(.pulse, options: .repeating)

            Group {
                if let label = toolStatusLabel {
                    toolStatusView(label: label)
                } else {
                    dotsView
                }
            }
            .padding(.horizontal, Layout.typingPaddingH)
            .padding(.vertical, Layout.typingPaddingV)
            .glassEffect(.regular, in: .rect(cornerRadius: Layout.typingCornerRadius))

            Spacer(minLength: 0)
        }
        .animation(AppTheme.Animation.springFast, value: toolName)
    }

    // MARK: - Dots

    private var dotsView: some View {
        HStack(spacing: Layout.typingDotSpacing) {
            ForEach(0..<3, id: \.self) { i in
                Circle()
                    .fill(.secondary)
                    .frame(width: Layout.typingDotSize, height: Layout.typingDotSize)
                    .opacity(phase == i ? 1.0 : 0.35)
                    .scaleEffect(phase == i ? 1.15 : 0.9)
                    .animation(AppTheme.Animation.easeInOut, value: phase)
            }
        }
        .task {
            while !Task.isCancelled {
                try? await Task.sleep(for: AppTheme.Timing.typingDotStep)
                phase = (phase + 1) % 3
            }
        }
    }

    // MARK: - Tool status label

    private func toolStatusView(label: String) -> some View {
        HStack(spacing: AppTheme.Spacing.xs) {
            Image(systemName: "wand.and.stars")
                .font(.system(size: Layout.batchIconSize, weight: .semibold))
                .foregroundStyle(AppTheme.Tint.agentSurface)
                .symbolEffect(.pulse, options: .repeating)
                .accessibilityHidden(true)
            Text(label)
                .font(AppTheme.Typography.caption)
                .foregroundStyle(.secondary)
        }
    }

    /// Maps a raw tool name to a concise human-readable present-progressive label.
    private var toolStatusLabel: String? {
        switch toolName {
        case AgentTools.Names.createNote:    return "Saving note…"
        case AgentTools.Names.recordMemory:  return "Saving memory…"
        default:
            guard let name = toolName else { return nil }
            // Fallback: prettify by replacing underscores with spaces and capitalising.
            return name.replacingOccurrences(of: "_", with: " ").capitalized + "…"
        }
    }
}
