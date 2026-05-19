import SwiftUI

/// "What changed" review sheet — shown from any UI that hands in a `batchID`
/// (e.g. when the user taps a tool-call batch row in the agent chat).
/// Lists the entries the agent produced for that batch with per-row Undo
/// + an Undo-all action.
///
/// The view reads `store.state.agentActivity` directly so per-row toggles
/// flow back through the existing @Observable pipeline.
struct AgentActivitySheet: View {

    // MARK: - Layout constants

    private enum Layout {
        /// Width reserved for each row's leading icon column.
        static let iconColumnWidth: CGFloat = 22
        /// Tight spacing between a summary and its timestamp sub-label.
        static let rowVerticalSpacing: CGFloat = 2
    }

    let batchID: UUID

    @Environment(AppStateStore.self) private var store
    @Environment(\.dismiss) private var dismiss

    private var entries: [AgentActivityEntry] {
        store.agentActivity(forBatch: batchID)
    }

    private var activeCount: Int {
        entries.filter { !$0.undone }.count
    }

    var body: some View {
        NavigationStack {
            Group {
                if entries.isEmpty {
                    emptyState
                } else {
                    list
                }
            }
            .navigationTitle("What changed")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
                ToolbarItem(placement: .primaryAction) {
                    Button("Undo all") {
                        withAnimation(AppTheme.Animation.spring) {
                            store.undoAgentActivityBatch(batchID)
                        }
                        Haptics.warning()
                    }
                    .disabled(activeCount == 0)
                }
            }
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }

    // MARK: - Subviews

    private var list: some View {
        List {
            Section {
                ForEach(entries) { entry in
                    row(for: entry)
                        .listRowSeparator(.hidden)
                }
            } header: {
                HStack(spacing: AppTheme.Spacing.xs) {
                    Image(systemName: "sparkles")
                        .foregroundStyle(.secondary)
                    Text("\(activeCount) of \(entries.count) still applied")
                        .font(AppTheme.Typography.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                }
                .textCase(nil)
            } footer: {
                Text("Tap Undo on any row to reverse just that change. The agent's other actions stay applied.")
                    .font(AppTheme.Typography.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
        .listStyle(.insetGrouped)
        .animation(AppTheme.Animation.spring, value: entries.map(\.undone))
    }

    @ViewBuilder
    private func row(for entry: AgentActivityEntry) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: AppTheme.Spacing.md) {
            Image(systemName: entry.kind.icon)
                .foregroundStyle(entry.undone ? AnyShapeStyle(.tertiary) : AnyShapeStyle(entry.kind.tint))
                .font(AppTheme.Typography.callout)
                .frame(width: Layout.iconColumnWidth)

            VStack(alignment: .leading, spacing: Layout.rowVerticalSpacing) {
                Text(entry.summary)
                    .font(AppTheme.Typography.body)
                    .foregroundStyle(entry.undone ? .secondary : .primary)
                    .strikethrough(entry.undone, color: .secondary)
                    .lineLimit(2)
                Text(RelativeTimestamp.compact(entry.timestamp))
                    .font(AppTheme.Typography.caption2)
                    .foregroundStyle(.tertiary)
                    .monospacedDigit()
            }

            Spacer(minLength: AppTheme.Spacing.sm)

            if entry.undone {
                Label("Undone", systemImage: "arrow.uturn.backward.circle.fill")
                    .labelStyle(.iconOnly)
                    .foregroundStyle(.secondary)
                    .font(AppTheme.Typography.title3)
                    .accessibilityLabel("Undone")
                    .transition(.scale.combined(with: .opacity))
            } else {
                Button("Undo") {
                    withAnimation(AppTheme.Animation.spring) {
                        store.undoAgentActivity(entry.id)
                    }
                    Haptics.selection()
                }
                .buttonStyle(.glass)
                .controlSize(.small)
                .transition(.opacity)
            }
        }
        .padding(.vertical, AppTheme.Spacing.xs)
        .contentShape(Rectangle())
    }

    private var emptyState: some View {
        ContentUnavailableView {
            Label("Nothing changed", systemImage: "sparkle")
        } description: {
            Text("The agent finished without making any changes.")
        }
    }

}

// MARK: - Activity kind presentation

extension AgentActivityKind {
    var icon: String {
        switch self {
        case .noteCreated: "note.text"
        case .memoryRecorded: "brain"
        }
    }

    var tint: Color {
        switch self {
        case .noteCreated: .purple
        case .memoryRecorded: .indigo
        }
    }
}
