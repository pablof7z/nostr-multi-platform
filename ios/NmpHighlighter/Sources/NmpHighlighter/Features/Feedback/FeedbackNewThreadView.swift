import SwiftUI

/// Body-only composer for a new feedback thread. Title comes later from the
/// agent's kind:513 metadata; until then the thread row falls back to the
/// trimmed body content.
struct FeedbackNewThreadView: View {
    let store: FeedbackStore
    /// Called after a successful send. Caller decides whether to also dismiss
    /// the parent threads sheet (currently always `false` — we stay in the
    /// list so the user sees their new thread arrive).
    let onSent: (Bool) -> Void

    @Environment(HighlighterStore.self) private var app
    @Environment(\.dismiss) private var dismiss

    @State private var draft: String = ""
    @State private var isPublishing = false
    @State private var errorMessage: String?

    private var canPublish: Bool {
        !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !isPublishing
    }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 0) {
                TextEditor(text: $draft)
                    .font(.body)
                    .padding(.horizontal, 12)
                    .padding(.top, 8)
                    .overlay(alignment: .topLeading) {
                        if draft.isEmpty {
                            Text("What's on your mind?")
                                .font(.body)
                                .foregroundStyle(.tertiary)
                                .padding(.horizontal, 17)
                                .padding(.top, 16)
                                .allowsHitTesting(false)
                        }
                    }
                if let errorMessage {
                    Text(errorMessage)
                        .font(.caption)
                        .foregroundStyle(.red)
                        .padding(.horizontal, 16)
                        .padding(.bottom, 8)
                }
            }
            .navigationTitle("New feedback")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .disabled(isPublishing)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(isPublishing ? "Sending…" : "Send") {
                        Task { await publish() }
                    }
                    .disabled(!canPublish)
                }
            }
        }
    }

    private func publish() async {
        isPublishing = true
        errorMessage = nil
        defer { isPublishing = false }

        do {
            let agent = await store.resolveAgentPubkey()
            let record = try await app.safeCore.publishFeedbackNote(
                coordinate: FeedbackProject.coordinate,
                agentPubkey: agent,
                parentEventId: nil,
                body: draft
            )
            store.optimisticallyInsert(rootEvent: record)
            await store.refreshThreads()
            dismiss()
            onSent(false)
        } catch {
            errorMessage =
                (error as? LocalizedError)?.errorDescription
                ?? (error as? CoreError).map { "\($0)" }
                ?? "\(error)"
        }
    }
}
