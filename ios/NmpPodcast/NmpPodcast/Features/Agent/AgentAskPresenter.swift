import SwiftUI

/// Watches an `AgentAskCoordinator` for pending owner-consultation
/// requests and auto-presents a sheet whenever the agent calls its
/// `ask` tool. Mounted once on `RootView` (alongside
/// `nostrApprovalPresenter()`) so the prompt can interrupt any tab —
/// critical for peer-agent flows where an inbound Nostr message can
/// trigger an `ask` while the user is on Home, Library, etc.
///
/// The sheet shows the head of the coordinator's FIFO queue. Each
/// resolve / decline pops the head and reveals the next pending ask.
struct AgentAskPresenter: ViewModifier {

    let coordinator: AgentAskCoordinator

    func body(content: Content) -> some View {
        content
            .sheet(item: Binding(
                get: { coordinator.current },
                set: { newValue in
                    // SwiftUI sets this to nil on swipe-dismiss. Treat
                    // that as an implicit decline; the coordinator's
                    // idempotency guard handles the case where the user
                    // tapped Decline explicitly first.
                    if newValue == nil, let current = coordinator.current {
                        coordinator.decline(current.id)
                    }
                }
            )) { ask in
                AgentAskSheet(coordinator: coordinator, ask: ask)
                    .presentationDetents([.medium, .large])
                    .presentationDragIndicator(.visible)
            }
    }
}

extension View {
    /// Attaches the global agent-ask sheet. Mount once high in the view
    /// hierarchy (e.g. `RootView`) so any tab can be interrupted when
    /// the agent needs to consult the owner mid-turn.
    func agentAskPresenter(coordinator: AgentAskCoordinator) -> some View {
        modifier(AgentAskPresenter(coordinator: coordinator))
    }
}

// MARK: - Sheet

/// Owner-facing surface for the agent's `ask` tool. Simple text-field
/// answer plus Decline — the win-the-day variant uses a realtime STT
/// flow, but the podcast app keeps the first cut text-based and stays
/// well under the file-length limits. Voice-answer parity can layer
/// on later via `VoiceNoteRealtimeSTT` without changing the contract.
///
/// Resolution invariants:
/// - Send with non-empty text → `coordinator.resolve`.
/// - "Decline" button → `coordinator.decline`.
/// - Swipe-dismiss → `coordinator.decline` via the presenter's
///   `.sheet(item:)` set-to-nil hook. The coordinator no-ops a second
///   resolve, so an explicit Decline immediately followed by a
///   dismiss-driven decline is safe.
private struct AgentAskSheet: View {

    let coordinator: AgentAskCoordinator
    let ask: AgentAskCoordinator.PendingAsk

    @State private var answer: String = ""
    @State private var resolved = false
    @FocusState private var inputFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: AppTheme.Spacing.md) {
            header
            questionCard
            answerField
            Spacer(minLength: AppTheme.Spacing.sm)
            actionButtons
        }
        .padding(AppTheme.Spacing.lg)
        .onAppear { inputFocused = true }
        .onDisappear {
            // Belt-and-suspenders: if the sheet disappears without
            // resolve having fired (e.g. system dismissed it), make sure
            // the continuation still completes. The coordinator's
            // idempotency guard collapses the second call.
            if !resolved {
                coordinator.decline(ask.id)
            }
        }
    }

    // MARK: - Sections

    private var header: some View {
        HStack(spacing: AppTheme.Spacing.sm) {
            Image(systemName: "sparkle")
                .font(.system(size: 22, weight: .semibold))
                .foregroundStyle(.tint)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: AppTheme.Spacing.xs) {
                Text("Your agent asks")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.tint)
                Text("Answer to let it continue, or decline.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
        }
    }

    private var questionCard: some View {
        VStack(alignment: .leading, spacing: AppTheme.Spacing.xs) {
            Text(ask.question)
                .font(.title3.weight(.semibold))
                .foregroundStyle(.primary)
                .multilineTextAlignment(.leading)
                .frame(maxWidth: .infinity, alignment: .leading)
            if let context = ask.context {
                Text(context)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.leading)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(AppTheme.Spacing.md)
        .background(
            RoundedRectangle(cornerRadius: AppTheme.Corner.md, style: .continuous)
                .fill(.thinMaterial)
        )
    }

    private var answerField: some View {
        VStack(alignment: .leading, spacing: AppTheme.Spacing.xs) {
            Text("Your answer")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            TextField("Type your reply…", text: $answer, axis: .vertical)
                .textFieldStyle(.plain)
                .focused($inputFocused)
                .lineLimit(2...6)
                .padding(AppTheme.Spacing.sm)
                .background(
                    RoundedRectangle(cornerRadius: AppTheme.Corner.md, style: .continuous)
                        .fill(Color(.secondarySystemBackground))
                )
                .submitLabel(.send)
                .onSubmit(sendAnswer)
        }
    }

    private var actionButtons: some View {
        HStack(spacing: AppTheme.Spacing.md) {
            Button(role: .destructive) {
                decline()
            } label: {
                Label("Decline", systemImage: "hand.raised")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered)

            Button {
                sendAnswer()
            } label: {
                Label("Send", systemImage: "arrow.up.circle.fill")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.glassProminent)
            .disabled(trimmedAnswer.isEmpty)
        }
    }

    // MARK: - Actions

    private var trimmedAnswer: String {
        answer.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func sendAnswer() {
        let trimmed = trimmedAnswer
        guard !trimmed.isEmpty else { return }
        resolved = true
        Haptics.success()
        coordinator.resolve(ask.id, with: trimmed)
    }

    private func decline() {
        resolved = true
        Haptics.selection()
        coordinator.decline(ask.id)
    }
}
