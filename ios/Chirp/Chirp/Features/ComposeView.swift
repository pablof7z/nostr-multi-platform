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
                    VStack(alignment: .leading, spacing: ChirpSpace.m) {
                        // Reply context banner
                        if let replyToID {
                            replyBanner(for: replyToID)
                        }

                        // Compose area
                        GlassCard {
                            VStack(alignment: .leading, spacing: ChirpSpace.s) {
                                TextEditor(text: $text)
                                    .focused($editorFocused)
                                    .font(ChirpFont.body)
                                    .foregroundStyle(ChirpColor.textPrimary)
                                    .scrollContentBackground(.hidden)
                                    .background(Color.clear)
                                    .frame(minHeight: 140)
                                    .overlay(alignment: .topLeading) {
                                        if text.isEmpty {
                                            Text(isReply ? "Write your reply…" : "What's happening?")
                                                .font(ChirpFont.body)
                                                .foregroundStyle(ChirpColor.textTertiary)
                                                .allowsHitTesting(false)
                                                .padding(.top, 8)
                                                .padding(.leading, 4)
                                        }
                                    }

                                Divider()
                                    .background(ChirpColor.hairline)

                                // Character counter
                                HStack {
                                    Spacer()
                                    Text("\(charRemaining)")
                                        .font(ChirpFont.caption)
                                        .foregroundStyle(
                                            isOverLimit ? ChirpColor.like :
                                            charRemaining <= 20 ? ChirpColor.zap :
                                            ChirpColor.textTertiary
                                        )
                                        .contentTransition(.numericText())
                                        .animation(.smooth(duration: 0.2), value: charRemaining)
                                }
                            }
                        }
                        .padding(.horizontal, ChirpSpace.l)

                        // Post button — inside scroll so it's above keyboard
                        ChirpPrimaryButton(
                            title: isReply ? "Reply" : "Post",
                            systemImage: isReply ? "arrowshape.turn.up.left.fill" : "paperplane.fill"
                        ) {
                            model.publishNote(trimmed, replyToID: replyToID)
                            dismiss()
                        }
                        .disabled(isEmpty || isOverLimit)
                        .opacity(isEmpty || isOverLimit ? 0.45 : 1.0)
                        .animation(.smooth(duration: 0.2), value: isEmpty)
                        .padding(.horizontal, ChirpSpace.l)
                        .padding(.bottom, ChirpSpace.xl)
                    }
                    .padding(.top, ChirpSpace.l)
                }
            }
            .navigationTitle(isReply ? "Reply" : "New Post")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel") { dismiss() }
                        .font(ChirpFont.callout)
                        .foregroundStyle(ChirpColor.textSecondary)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    // Character arc indicator for the remaining chars
                    if !text.isEmpty {
                        CharacterArcView(remaining: charRemaining, limit: characterLimit)
                            .frame(width: 24, height: 24)
                    }
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
        HStack(spacing: ChirpSpace.s) {
            Image(systemName: "arrowshape.turn.up.left.fill")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(ChirpColor.accent)

            Text("Replying to a note")
                .font(ChirpFont.callout)
                .foregroundStyle(ChirpColor.textSecondary)

            Spacer()

            Text(shortID(id))
                .font(ChirpFont.mono)
                .foregroundStyle(ChirpColor.textTertiary)
        }
        .padding(.horizontal, ChirpSpace.l)
        .padding(.vertical, ChirpSpace.s)
        .background(ChirpColor.accentSoft, in: RoundedRectangle(
            cornerRadius: ChirpSpace.radiusSmall, style: .continuous))
        .padding(.horizontal, ChirpSpace.l)
    }

    private func shortID(_ id: String) -> String {
        guard id.count >= 12 else { return id }
        return "\(id.prefix(6))…\(id.suffix(4))"
    }
}

// ── Character arc — thin progress ring that fills as limit approaches ─────

private struct CharacterArcView: View {
    let remaining: Int
    let limit: Int

    private var progress: Double {
        let used = limit - remaining
        return Double(max(0, min(used, limit))) / Double(limit)
    }

    private var ringColor: Color {
        if remaining < 0 { return ChirpColor.like }
        if remaining <= 20 { return ChirpColor.zap }
        return ChirpColor.accent
    }

    var body: some View {
        ZStack {
            Circle()
                .stroke(ChirpColor.hairline.opacity(0.5), lineWidth: 2.5)
            Circle()
                .trim(from: 0, to: progress)
                .stroke(ringColor, style: StrokeStyle(lineWidth: 2.5, lineCap: .round))
                .rotationEffect(.degrees(-90))
                .animation(.smooth(duration: 0.15), value: progress)
        }
    }
}
