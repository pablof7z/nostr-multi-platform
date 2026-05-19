import SwiftUI

// OWNER: Phase-2 Agent C (Compose sheet). Presented as a sheet from
// HomeFeedView / NoteRowView. Supports ComposeView() and
// ComposeView(replyToID: "abc").

struct ComposeView: View {
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    var replyToID: String? = nil

    @State private var text = ""
    @FocusState private var editorFocused: Bool

    private let characterLimit = 280

    private var isReply: Bool { replyToID != nil }
    private var trimmed: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var isEmpty: Bool { trimmed.isEmpty }
    private var charCount: Int { text.count }
    private var charRemaining: Int { characterLimit - charCount }
    private var isOverLimit: Bool { charCount > characterLimit }

    var body: some View {
        NavigationStack {
            ZStack(alignment: .bottom) {
                // Background
                Color(.systemBackground)
                    .ignoresSafeArea()

                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        // Reply context banner
                        if let replyToID {
                            replyBanner(for: replyToID)
                        }

                        // Compose area
                        VStack(alignment: .leading, spacing: 8) {
                            TextEditor(text: $text)
                                .focused($editorFocused)
                                .font(.body)
                                .foregroundStyle(.primary)
                                .frame(minHeight: 140)
                                .overlay(alignment: .topLeading) {
                                    if text.isEmpty {
                                        Text(isReply ? "Write your reply…" : "What's happening?")
                                            .font(.body)
                                            .foregroundStyle(.secondary)
                                            .allowsHitTesting(false)
                                            .padding(.top, 8)
                                            .padding(.leading, 4)
                                    }
                                }

                            Divider()

                            // Character counter
                            HStack {
                                Spacer()
                                Text("\(charRemaining)")
                                    .font(.caption)
                                    .foregroundStyle(
                                        isOverLimit ? .red :
                                        charRemaining <= 20 ? .orange :
                                        .secondary
                                    )
                                    .contentTransition(.numericText())
                                    .animation(.smooth(duration: 0.2), value: charRemaining)
                            }
                        }
                        .padding(.horizontal, 16)

                        // Post button — inside scroll so it's above keyboard
                        Button {
                            model.publishNote(trimmed, replyToID: replyToID)
                            dismiss()
                        } label: {
                            HStack {
                                Image(systemName: isReply ? "arrowshape.turn.up.left.fill" : "paperplane.fill")
                                Text(isReply ? "Reply" : "Post")
                            }
                            .font(.headline)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 12)
                        }
                        .disabled(isEmpty || isOverLimit)
                        .opacity(isEmpty || isOverLimit ? 0.45 : 1.0)
                        .animation(.smooth(duration: 0.2), value: isEmpty)
                        .padding(.horizontal, 16)
                        .padding(.bottom, 32)
                    }
                    .padding(.top, 16)
                }
            }
            .navigationTitle(isReply ? "Reply" : "New Post")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
        .onAppear {
            // Small delay so the sheet fully presents before the keyboard appears
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) {
                editorFocused = true
            }
        }
    }

    @ViewBuilder
    private func replyBanner(for id: String) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "arrowshape.turn.up.left.fill")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(Color.accentColor)

            Text("Replying to a note")
                .font(.callout)
                .foregroundStyle(.secondary)

            Spacer()

            Text(shortID(id))
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .background(Color(.secondarySystemBackground))
        .padding(.horizontal, 16)
    }

    private func shortID(_ id: String) -> String {
        guard id.count >= 12 else { return id }
        return "\(id.prefix(6))…\(id.suffix(4))"
    }
}
