import SwiftUI

/// Toolbar button that bundles the simple bookmark toggle (kind:10003)
/// with a long-press menu that lets the user add the same artifact to
/// any of their kind:30004 curation sets — including a "New
/// collection…" entry that prompts for a title and adds the artifact in
/// one shot.
///
/// Uses SwiftUI's `Menu(primaryAction:)` so a tap stays one-tap-fast and
/// long-press surfaces the curation choices. Loads curations lazily on
/// the first appear; refreshes after every membership change so the
/// checkmark state is always accurate without a full BookmarkStore.
struct BookmarkMenuButton: View {
    /// NIP-33 a-tag value — `"30023:<pubkey>:<d>"`.
    let articleAddress: String

    @Environment(HighlighterStore.self) private var app

    @State private var curationSets: [BookmarkSetRecord] = []
    @State private var newCollectionPresented: Bool = false
    @State private var errorMessage: String?

    var body: some View {
        Menu {
            curationsSection
            Divider()
            Button {
                newCollectionPresented = true
            } label: {
                Label("New collection…", systemImage: "plus")
            }
        } label: {
            Image(systemName: isBookmarked ? "bookmark.fill" : "bookmark")
                .foregroundStyle(isBookmarked ? Color.highlighterAccent : Color.highlighterInkStrong)
        } primaryAction: {
            Task { await app.toggleBookmark(articleAddress: articleAddress) }
        }
        .accessibilityLabel(isBookmarked ? "Remove bookmark" : "Bookmark article")
        .task { await loadCurations() }
        .sheet(isPresented: $newCollectionPresented) {
            NewCollectionSheet(
                onCancel: { newCollectionPresented = false },
                onCreate: { title in
                    newCollectionPresented = false
                    Task { await createAndAdd(title: title) }
                }
            )
            .presentationDetents([.medium])
        }
    }

    @ViewBuilder
    private var curationsSection: some View {
        if curationSets.isEmpty {
            // Header-only section so the menu still reads as the
            // collection picker before any sets exist.
            Text("No collections yet")
                .font(.footnote)
        } else {
            Section("Add to collection") {
                ForEach(curationSets, id: \.id) { set in
                    Button {
                        Task { await toggleInCuration(set) }
                    } label: {
                        if set.articleAddresses.contains(articleAddress) {
                            Label(displayTitle(set), systemImage: "checkmark")
                        } else {
                            Text(displayTitle(set))
                        }
                    }
                }
            }
        }
    }

    private var isBookmarked: Bool {
        app.isBookmarked(articleAddress: articleAddress)
    }

    private func displayTitle(_ set: BookmarkSetRecord) -> String {
        if !set.title.isEmpty { return set.title }
        if !set.id.isEmpty { return set.id }
        return "Untitled"
    }

    // MARK: - Actions

    private func loadCurations() async {
        if let sets = try? await app.safeCore.getMyCurationSets() {
            curationSets = sets.sorted { ($0.createdAt ?? 0) > ($1.createdAt ?? 0) }
        }
    }

    private func toggleInCuration(_ set: BookmarkSetRecord) async {
        let nowMember = !set.articleAddresses.contains(articleAddress)
        do {
            try await app.safeCore.setAddressInCurationSet(
                dTag: set.id,
                address: articleAddress,
                member: nowMember
            )
            await loadCurations()
        } catch {
            errorMessage = "Couldn't update collection — \(error.localizedDescription)"
        }
    }

    private func createAndAdd(title: String) async {
        do {
            let newSet = try await app.safeCore.createCurationSet(title: title)
            try await app.safeCore.setAddressInCurationSet(
                dTag: newSet.id,
                address: articleAddress,
                member: true
            )
            await loadCurations()
        } catch {
            errorMessage = "Couldn't create collection — \(error.localizedDescription)"
        }
    }
}

/// Tiny modal that prompts for a new collection title. Cancel discards;
/// Save invokes `onCreate(title)`. Title field is auto-focused so the
/// keyboard shows immediately.
struct NewCollectionSheet: View {
    var onCancel: () -> Void
    var onCreate: (String) -> Void

    @State private var title: String = ""
    @FocusState private var focused: Bool

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 16) {
                Text("Group articles, podcasts, or notes you want to share or revisit. You can add to it from any artifact.")
                    .font(.footnote)
                    .foregroundStyle(Color.highlighterInkMuted)

                TextField("Collection name", text: $title)
                    .focused($focused)
                    .textFieldStyle(.roundedBorder)
                    .submitLabel(.done)
                    .onSubmit { commit() }

                Spacer(minLength: 0)
            }
            .padding(.horizontal, 20)
            .padding(.top, 16)
            .navigationTitle("New collection")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Cancel", action: onCancel)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Save") { commit() }
                        .fontWeight(.semibold)
                        .disabled(trimmed.isEmpty)
                }
            }
            .onAppear { focused = true }
        }
    }

    private var trimmed: String {
        title.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func commit() {
        guard !trimmed.isEmpty else { return }
        onCreate(trimmed)
    }
}
