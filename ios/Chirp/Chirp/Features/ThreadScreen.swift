import SwiftUI

// OWNER: Phase-2 Agent B (Thread screen).
// Type name is ThreadScreen — Bridge already defines a Decodable `ThreadView`.
// Init signature FIXED by nav contract: ThreadScreen(eventID:).

struct ThreadScreen: View {
    let eventID: String

    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    /// The event ID we want to present a reply compose sheet for.
    @State private var replyTargetID: ReplyTarget? = nil

    private var thread: ThreadView? { model.threadView }

    var body: some View {
        Group {
            if let thread {
                threadContent(thread)
            } else {
                loadingState
            }
        }
        .background(ChirpColor.bg.ignoresSafeArea())
        .navigationTitle("Thread")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            model.openThread(eventID: eventID)
        }
        .onDisappear {
            // T152: release the thread subscription when this view is no
            // longer visible.  Symmetric with openThread in .task above.
            model.closeThread(eventID: eventID)
        }
        .animation(.smooth(duration: 0.3), value: thread == nil)
        .sheet(item: $replyTargetID) { target in
            ComposeView(replyToID: target.eventID)
        }
    }

    // MARK: – Loading state

    private var loadingState: some View {
        VStack(spacing: ChirpSpace.xl) {
            ChirpPlaceholder(
                systemImage: "bubble.left.and.bubble.right",
                title: "Loading thread…",
                subtitle: "Fetching notes from the relay network."
            )
            .frame(maxHeight: 320)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: – Thread content

    @ViewBuilder
    private func threadContent(_ thread: ThreadView) -> some View {
        ScrollViewReader { proxy in
        ScrollView {
            LazyVStack(spacing: 0) {
                // "Show N earlier" affordance
                if thread.previousCount > 0 {
                    earlierAffordance(count: thread.previousCount)
                }

                // All thread items — focused one is highlighted
                ForEach(thread.items) { item in
                    let isFocused = item.id == thread.focusedEventId

                    ThreadNoteRow(
                        item: item,
                        isFocused: isFocused,
                        onAvatarTap: {
                            router.push(.profile(pubkey: item.authorPubkey))
                        },
                        onLike: {
                            model.react(targetEventID: item.id, reaction: "❤")
                        },
                        onReply: {
                            replyTargetID = ReplyTarget(eventID: item.id)
                        }
                    )
                    .id(item.id)
                    .accessibilityIdentifier(isFocused ? "thread-focused-note" : "thread-note-\(item.id.prefix(8))")

                    // Thread connector line between non-focused notes
                    if item.id != thread.items.last?.id {
                        threadConnector(isFocused: isFocused)
                    }
                }

                // More replies below affordance
                if thread.nextCount > 0 {
                    HStack(spacing: ChirpSpace.s) {
                        Image(systemName: "ellipsis.bubble")
                            .font(.system(size: 13))
                        Text("\(thread.nextCount) more repl\(thread.nextCount == 1 ? "y" : "ies")")
                            .font(ChirpFont.callout)
                    }
                    .foregroundStyle(ChirpColor.textTertiary)
                    .padding(.vertical, ChirpSpace.m)
                    .padding(.horizontal, ChirpSpace.l)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                Spacer(minLength: ChirpSpace.xxl)
            }
        }
        .accessibilityIdentifier("thread-detail-list")
        .onAppear {
            // Scroll to focused event once view appears
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
                withAnimation(.smooth(duration: 0.4)) {
                    proxy.scrollTo(thread.focusedEventId, anchor: .center)
                }
            }
        }
        } // ScrollViewReader
    }

    // MARK: – Sub-views

    private func earlierAffordance(count: Int) -> some View {
        HStack(spacing: ChirpSpace.s) {
            Image(systemName: "arrow.up.circle")
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(ChirpColor.accent)
            Text("Show \(count) earlier \(count == 1 ? "note" : "notes")")
                .font(ChirpFont.callout)
                .foregroundStyle(ChirpColor.accent)
            Spacer()
        }
        .padding(.vertical, ChirpSpace.m)
        .padding(.horizontal, ChirpSpace.l)
        .background(ChirpColor.accentSoft)
        .contentShape(Rectangle())
        .onTapGesture {
            // No kernel command exists to expand context yet — haptic feedback only.
            let g = UIImpactFeedbackGenerator(style: .light)
            g.impactOccurred()
        }
    }

    @ViewBuilder
    private func threadConnector(isFocused: Bool) -> some View {
        HStack {
            // Align with avatar leading edge
            Spacer()
                .frame(width: ChirpSpace.l + (isFocused ? 46 : 38) / 2 - 1)
            Rectangle()
                .fill(isFocused ? ChirpColor.accent.opacity(0.4) : ChirpColor.hairline)
                .frame(width: 2, height: ChirpSpace.m)
                .cornerRadius(1)
            Spacer()
        }
    }
}

// MARK: – Lightweight wrapper used for sheet(item:) presentation

private struct ReplyTarget: Identifiable {
    let eventID: String
    var id: String { eventID }
}
