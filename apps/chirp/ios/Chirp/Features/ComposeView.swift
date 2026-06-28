import SwiftUI

// OWNER: Phase-2 Agent C (Compose sheet). Presented as a sheet from
// HomeFeedView / NoteRowView. Supports ComposeView() and
// ComposeView(replyTo: ChirpReplyTarget(row: row)).

struct ComposeView: View {
    @EnvironmentObject private var model: KernelModel
    @Environment(\.dismiss) private var dismiss

    var replyTo: ChirpReplyTarget? = nil

    @State private var text = ""
    @FocusState private var editorFocused: Bool

    private let characterLimit = 280

    private var isReply: Bool { replyTo != nil }
    private var trimmed: String { text.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var isEmpty: Bool { trimmed.isEmpty }
    private var charCount: Int { text.count }
    private var charRemaining: Int { characterLimit - charCount }
    private var isOverLimit: Bool { charCount > characterLimit }
    private var currentAccount: AccountSummary? {
        guard let activeID = model.activeAccount else { return nil }
        return model.accounts.first { $0.id == activeID }
    }
    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                if let replyTo {
                    replyBanner(shortID: replyTo.eventID.shortHex)
                }

                composeRow
                Spacer(minLength: 0)
                composeFooter
            }
            .background(ChirpColor.bg.ignoresSafeArea())
            .navigationTitle(isReply ? "Reply" : "New Post")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                }

                ToolbarItem(placement: .topBarTrailing) {
                    Button(isReply ? "Reply" : "Post", action: submit)
                        .fontWeight(.semibold)
                        .disabled(isEmpty || isOverLimit)
                        .accessibilityIdentifier("compose-post-button")
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

    private var composeRow: some View {
        HStack(alignment: .top, spacing: 12) {
            currentAvatar
            editorStack
        }
        .padding(16)
    }

    @ViewBuilder
    private var currentAvatar: some View {
        if let account = currentAccount {
            NostrAvatar(
                pubkey: account.id,
                url: account.pictureUrl,
                size: 42
            )
            .equatable()
        } else {
            Image(systemName: "person.crop.circle.fill")
                .font(.system(size: 42))
                .foregroundStyle(.secondary)
        }
    }

    private var editorStack: some View {
        ZStack(alignment: .topLeading) {
            TextEditor(text: $text)
                .focused($editorFocused)
                .font(.body)
                .foregroundStyle(.primary)
                .scrollContentBackground(.hidden)
                .frame(minHeight: 190)
                .accessibilityIdentifier("compose-text-editor")

            if text.isEmpty {
                Text(isReply ? "Write your reply…" : "What's happening?")
                    .font(.body)
                    .foregroundStyle(.secondary)
                    .allowsHitTesting(false)
                    .padding(.top, 8)
                    .padding(.leading, 5)
            }
        }
    }

    @ViewBuilder
    private var composeFooter: some View {
        // The character-count ring only earns space once the user is typing —
        // an empty compose sheet should be a clean editor, not a stray ring.
        if charCount > 0 {
            HStack {
                Spacer()
                ComposeProgressRing(used: charCount, limit: characterLimit)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .overlay(alignment: .top) {
                Divider()
            }
        }
    }

    @ViewBuilder
    private func replyBanner(shortID: String) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "arrowshape.turn.up.left.fill")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(ChirpColor.accent)

            Text("Replying to a note")
                .font(.callout)
                .foregroundStyle(.secondary)

            Spacer()

            Text(shortID)
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(ChirpColor.surface)
        .overlay(alignment: .bottom) {
            Divider()
        }
    }

    private func submit() {
        model.publishNote(trimmed, replyTo: replyTo)
        UINotificationFeedbackGenerator().notificationOccurred(.success)
        dismiss()
    }
}
