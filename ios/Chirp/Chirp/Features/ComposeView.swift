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
    private var activeAccount: AccountSummary? {
        guard let activeID = model.activeAccount else { return nil }
        return model.accounts.first { $0.id == activeID }
    }

    var body: some View {
        NavigationStack {
            ZStack(alignment: .bottom) {
                ChirpBackdrop()

                ScrollView {
                    VStack(alignment: .leading, spacing: ChirpSpace.l) {
                        accountHeader

                        if let replyToID {
                            replyBanner(for: replyToID)
                        }

                        VStack(alignment: .leading, spacing: ChirpSpace.m) {
                            TextEditor(text: $text)
                                .focused($editorFocused)
                                .font(.body)
                                .foregroundStyle(.primary)
                                .scrollContentBackground(.hidden)
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

                            composeFooter
                        }
                        .padding(ChirpSpace.l)
                        .chirpGlass(cornerRadius: ChirpSpace.radius)
                    }
                    .padding(.horizontal, ChirpSpace.l)
                    .padding(.top, ChirpSpace.l)
                    .padding(.bottom, 110)
                }

                sendBar
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

    private var accountHeader: some View {
        HStack(spacing: ChirpSpace.m) {
            if let account = activeAccount {
                ChirpAvatar(
                    url: nil,
                    initials: account.avatarInitials,
                    colorHex: account.avatarColorHex,
                    size: 38
                )

                VStack(alignment: .leading, spacing: 2) {
                    Text(account.displayName.isEmpty ? "Posting account" : account.displayName)
                        .font(.callout.weight(.semibold))
                    Text(shortID(account.npub))
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            } else {
                Image(systemName: "person.crop.circle")
                    .font(.system(size: 34))
                    .foregroundStyle(.secondary)

                Text("No active account")
                    .font(.callout.weight(.semibold))
            }

            Spacer()
        }
        .padding(ChirpSpace.m)
        .chirpSurface(cornerRadius: ChirpSpace.radiusSmall)
        .accessibilityElement(children: .combine)
    }

    private var composeFooter: some View {
        VStack(spacing: ChirpSpace.s) {
            Divider()

            HStack(spacing: ChirpSpace.m) {
                Text(isReply ? "Reply" : "Public note")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                Spacer()

                Text("\(charRemaining)")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(counterColor)
                    .contentTransition(.numericText())
                    .animation(.smooth(duration: 0.2), value: charRemaining)
                    .accessibilityLabel("\(charRemaining) characters remaining")
            }
        }
    }

    private var sendBar: some View {
        HStack(spacing: ChirpSpace.m) {
            if !model.publishQueue.isEmpty {
                Label("\(model.publishQueue.count) pending", systemImage: "paperplane")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer()

            Button {
                model.publishNote(trimmed, replyToID: replyToID)
                dismiss()
            } label: {
                Label(isReply ? "Reply" : "Post", systemImage: isReply ? "arrowshape.turn.up.left.fill" : "paperplane.fill")
                    .frame(minWidth: 96)
            }
            .buttonStyle(ChirpGlassButtonStyle(prominent: true))
            .disabled(isEmpty || isOverLimit)
            .opacity(isEmpty || isOverLimit ? 0.45 : 1.0)
            .animation(.smooth(duration: 0.2), value: isEmpty)
        }
        .padding(.horizontal, ChirpSpace.l)
        .padding(.top, ChirpSpace.m)
        .padding(.bottom, ChirpSpace.l)
        .background(.regularMaterial)
    }

    private var counterColor: Color {
        if isOverLimit { return .red }
        if charRemaining <= 20 { return .orange }
        return .secondary
    }

    @ViewBuilder
    private func replyBanner(for id: String) -> some View {
        HStack(spacing: 8) {
            Image(systemName: "arrowshape.turn.up.left.fill")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(.tint)

            Text("Replying to a note")
                .font(.callout)
                .foregroundStyle(.secondary)

            Spacer()

            Text(shortID(id))
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)
        }
        .padding(ChirpSpace.m)
        .chirpSurface(cornerRadius: ChirpSpace.radiusSmall, muted: true)
    }

    private func shortID(_ id: String) -> String {
        guard id.count >= 12 else { return id }
        return "\(id.prefix(6))…\(id.suffix(4))"
    }
}
