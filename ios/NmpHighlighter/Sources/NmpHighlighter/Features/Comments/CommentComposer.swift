import SwiftUI

/// Pinned-bottom composer. Replies to the current thread's subject —
/// `parentEventId == nil` posts a top-level thread on the artifact;
/// otherwise posts as a reply. Drafts are kept in `CommentsStore` keyed
/// by parent so detent transitions don't lose typed text.
struct CommentComposer: View {
    let parentEventId: String?
    /// Display label for the composer placeholder — caller passes context
    /// like "Add to the conversation" at root, "Reply to @alice" inside
    /// a pushed thread.
    let placeholder: String

    let store: CommentsStore

    @FocusState private var focused: Bool
    @State private var isPublishing: Bool = false
    @State private var errorMessage: String?

    private var draft: Binding<String> {
        Binding(
            get: { store.draft(forParent: parentEventId) },
            set: { store.setDraft($0, forParent: parentEventId) }
        )
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let errorMessage {
                Text(errorMessage)
                    .font(.caption)
                    .foregroundStyle(Color.highlighterAccent)
                    .padding(.horizontal, 14)
                    .padding(.top, 6)
                    .transition(.opacity)
            }
            HStack(alignment: .bottom, spacing: 10) {
                TextField(placeholder, text: draft, axis: .vertical)
                    .focused($focused)
                    .lineLimit(1...6)
                    .font(.body)
                    .foregroundStyle(Color.highlighterInkStrong)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .background(
                        Color.highlighterInkStrong.opacity(0.06),
                        in: RoundedRectangle(cornerRadius: 18, style: .continuous)
                    )
                    .submitLabel(.send)
                    .onSubmit { submit() }

                sendButton
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
        }
        .background(
            .regularMaterial,
            in: RoundedRectangle(cornerRadius: 0, style: .continuous)
        )
        .overlay(alignment: .top) {
            Rectangle()
                .fill(Color.highlighterRule.opacity(0.6))
                .frame(height: 0.5)
        }
    }

    private var sendButton: some View {
        Button(action: submit) {
            ZStack {
                Circle()
                    .fill(canSubmit ? Color.highlighterAccent : Color.highlighterInkMuted.opacity(0.35))
                if isPublishing {
                    ProgressView()
                        .progressViewStyle(.circular)
                        .tint(.white)
                } else {
                    Image(systemName: "arrow.up")
                        .font(.system(size: 14, weight: .bold))
                        .foregroundStyle(.white)
                }
            }
            .frame(width: 36, height: 36)
        }
        .buttonStyle(.plain)
        .disabled(!canSubmit || isPublishing)
        .animation(.easeInOut(duration: 0.18), value: canSubmit)
    }

    private var canSubmit: Bool {
        !draft.wrappedValue.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private func submit() {
        guard canSubmit, !isPublishing else { return }
        isPublishing = true
        errorMessage = nil
        let text = draft.wrappedValue
        Task {
            do {
                _ = try await store.publish(content: text, parentEventId: parentEventId)
                isPublishing = false
                focused = false
            } catch {
                isPublishing = false
                let msg = (error as? CoreError).map { "\($0)" } ?? error.localizedDescription
                withAnimation(.easeOut(duration: 0.18)) {
                    errorMessage = "Couldn't publish — \(msg)"
                }
                try? await Task.sleep(nanoseconds: 2_400_000_000)
                withAnimation(.easeIn(duration: 0.18)) {
                    errorMessage = nil
                }
            }
        }
    }
}
