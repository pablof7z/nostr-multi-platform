import SwiftUI

/// Sheet presented on shake. Slack-style list of feedback threads the current
/// user has started, with a "New thread" entry point.
struct FeedbackThreadsView: View {
    @Environment(HighlighterStore.self) private var app
    @Environment(\.dismiss) private var dismiss
    @State private var store = FeedbackStore()
    @State private var composerPresented = false

    var body: some View {
        NavigationStack {
            content
                .navigationTitle("Feedback")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Done") { dismiss() }
                    }
                    ToolbarItem(placement: .confirmationAction) {
                        Button {
                            composerPresented = true
                        } label: {
                            Label("New thread", systemImage: "square.and.pencil")
                        }
                    }
                }
        }
        .task {
            await store.start(
                coordinate: FeedbackProject.coordinate,
                core: app.safeCore,
                bridge: app.eventBridge
            )
        }
        .onDisappear { store.stop() }
        .sheet(isPresented: $composerPresented) {
            FeedbackNewThreadView(store: store) { dismissThreadsAfter in
                if dismissThreadsAfter {
                    dismiss()
                }
            }
        }
    }

    @ViewBuilder
    private var content: some View {
        if store.isLoading && store.threads.isEmpty {
            ProgressView().controlSize(.large)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if let error = store.loadError, store.threads.isEmpty {
            ContentUnavailableView(
                "Couldn't load feedback",
                systemImage: "exclamationmark.triangle",
                description: Text(error)
            )
        } else if store.threads.isEmpty {
            ContentUnavailableView {
                Label("No feedback yet", systemImage: "bubble.left.and.bubble.right")
            } description: {
                Text("Tap the pencil to start a thread. Shake again any time to come back.")
            } actions: {
                Button("New thread") { composerPresented = true }
                    .buttonStyle(.borderedProminent)
            }
        } else {
            List(store.threads, id: \.rootEventId) { thread in
                NavigationLink {
                    FeedbackThreadDetailView(thread: thread, listStore: store)
                } label: {
                    FeedbackThreadRow(thread: thread)
                }
            }
            .listStyle(.plain)
        }
    }
}

private struct FeedbackThreadRow: View {
    let thread: FeedbackThreadRecord

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline) {
                Text(thread.title ?? thread.preview)
                    .font(.body.weight(.semibold))
                    .lineLimit(1)
                Spacer()
                Text(relativeTime(thread.lastActivityAt))
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            if let summary = thread.summary, !summary.isEmpty {
                Text(summary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            } else if thread.title != nil, !thread.preview.isEmpty {
                Text(thread.preview)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            if let status = thread.statusLabel, !status.isEmpty {
                Text(status.uppercased())
                    .font(.caption2.weight(.semibold))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.accentColor.opacity(0.15), in: .capsule)
                    .foregroundStyle(Color.accentColor)
            }
        }
        .padding(.vertical, 4)
    }

    private func relativeTime(_ ts: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(ts))
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter.localizedString(for: date, relativeTo: Date())
    }
}
