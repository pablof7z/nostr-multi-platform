import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// HomeFeedView — Home timeline root for Chirp.
//
// Renders `model.modularTimeline.blocks` (T146) using `ModularBlockView`:
// `Standalone` blocks delegate to the existing `NoteRowView`; `Module`
// blocks stack two-or-three events vertically with a connecting line in
// the avatar column. The flat `model.items` list is still around and is
// consumed by `ProfileView` / `ThreadScreen` (M2 follow-up migrates them).
//
// Empty state and pull-to-refresh stay unchanged. The blocks/cards lookup
// table is rebuilt every body pass — `[TimelineBlock]` and
// `[ChirpEventCard]` are small (≤ visible_limit; ≤80 by default), so the
// renderer doesn't need to memoize it.
// ─────────────────────────────────────────────────────────────────────────

struct HomeFeedView: View {
    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    /// Controls the top-level "new note" compose sheet (toolbar button).
    @State private var showCompose = false

    var body: some View {
        ZStack {
            if isEmpty {
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

    // T146 — empty when neither blocks nor the legacy flat list has
    // anything to render. The legacy fallback is the safety net for any
    // surface where the projection hasn't caught up yet (e.g. cold boot
    // before the first observer fan-out reaches Swift).
    private var isEmpty: Bool {
        model.modularTimeline.blocks.isEmpty && model.items.isEmpty
    }

    // ── Timeline list ──────────────────────────────────────────────────────

    private var timeline: some View {
        let blocks = effectiveBlocks
        let cardLookup = Dictionary(uniqueKeysWithValues: model.modularTimeline.cards.map { ($0.id, $0) })
        let itemLookup = Dictionary(uniqueKeysWithValues: model.items.map { ($0.id, $0) })

        return List {
            ForEach(Array(blocks.enumerated()), id: \.offset) { (_, block) in
                ModularBlockView(block: block, cards: cardLookup, items: itemLookup)
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(ChirpColor.bg)
        .animation(.smooth, value: blocks.count)
        .accessibilityIdentifier("timeline-list")
        .refreshable {
            model.openTimeline()
        }
    }

    // T146 — render modular blocks if any have been projected; otherwise
    // synthesize one Standalone block per `TimelineItem` so the cold-boot
    // window (before the projection has accepted its first event) still
    // shows the kernel's flat list. This is the only Swift-side fallback
    // in the modular-vs-flat split.
    private var effectiveBlocks: [TimelineBlock] {
        if !model.modularTimeline.blocks.isEmpty {
            return model.modularTimeline.blocks
        }
        return model.items.map { .standalone(eventID: $0.id) }
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
