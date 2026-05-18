import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// HomeFeedView — Home timeline root for Chirp.
//
// Responsibilities:
//   • Display model.items in a polished List (plain style, no UIKit chrome).
//   • Toolbar compose button (square.and.pencil) opens ComposeView sheet.
//   • Pull-to-refresh calls model.openTimeline() — idempotent cursor signal.
//   • .task on appear also calls openTimeline() for cold-launch correctness.
//   • Empty/loading state via ChirpPlaceholder so the screen is never blank.
//
// Navigation is entirely through ChirpRouter environment — no NavigationLink.
// ─────────────────────────────────────────────────────────────────────────

struct HomeFeedView: View {
    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    /// Controls the top-level "new note" compose sheet (toolbar button).
    @State private var showCompose = false

    var body: some View {
        ZStack {
            if model.items.isEmpty {
                emptyState
            } else {
                timeline
            }
        }
        .navigationTitle("Chirp")
        .navigationBarTitleDisplayMode(.large)
        .toolbar { composeButton }
        .task { model.openTimeline() }
        .sheet(isPresented: $showCompose) {
            ComposeView()
        }
    }

    // ── Timeline list ──────────────────────────────────────────────────────

    private var timeline: some View {
        List {
            ForEach(model.items) { item in
                NoteRowView(item: item)
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(ChirpColor.bg)
        .animation(.smooth, value: model.items.count)
        .refreshable {
            model.openTimeline()
        }
    }

    // ── Empty / loading state ─────────────────────────────────────────────

    private var emptyState: some View {
        ScrollView {
            ChirpPlaceholder(
                systemImage: "bird",
                title: "Your timeline",
                subtitle: "Loading your timeline…"
            )
            .frame(minHeight: 500)
        }
        .refreshable {
            model.openTimeline()
        }
    }

    // ── Toolbar: compose ──────────────────────────────────────────────────

    @ToolbarContentBuilder
    private var composeButton: some ToolbarContent {
        ToolbarItem(placement: .navigationBarTrailing) {
            Button {
                showCompose = true
            } label: {
                Image(systemName: "square.and.pencil")
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(ChirpColor.accent)
            }
            .buttonStyle(.borderless)
            .accessibilityLabel("New note")
        }
    }
}
