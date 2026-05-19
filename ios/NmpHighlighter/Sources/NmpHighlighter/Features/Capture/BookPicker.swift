import Kingfisher
import SwiftUI

/// One surface. No tabs. The user either taps a familiar cover (80%) or
/// reaches for scan/search (20%) — and the 20% moments are the ones that
/// have to feel magical.
///
/// Layout from top to bottom:
/// 1. Search-scan bar — one field. Typing filters; tapping the barcode glyph
///    opens the scanner in-place.
/// 2. Recents grid — 3-column covers, no text, the visual bed.
/// 3. Photo-only option — the escape hatch when the user doesn't want to
///    tag a book at all.
///
/// A successful scan or manual-ISBN entry opens a preview card as a sub-sheet
/// where the cover arrives and the user confirms "Use this book."
struct BookPicker: View {
    @Environment(HighlighterStore.self) private var appStore
    @Environment(\.dismiss) private var dismiss

    @Binding var selection: BookSelection?

    @State private var recents: [ArtifactRecord] = []
    @State private var searchResults: [ArtifactRecord] = []
    @State private var query: String = ""
    @State private var loadingRecents = true
    @State private var searching = false
    @State private var showScanner = false
    @State private var showManualEntry = false
    @State private var resolvingISBN: String?
    @State private var resolvedPreview: ArtifactPreview?
    @State private var resolveError: String?
    @FocusState private var searchFocused: Bool

    var body: some View {
        NavigationStack {
            ZStack {
                Color.highlighterPaper.ignoresSafeArea()
                ScrollView {
                    VStack(alignment: .leading, spacing: 24) {
                        searchScanBar
                        if !query.isEmpty {
                            searchResultsSection
                        } else {
                            recentsSection
                        }
                        photoOnlyRow
                    }
                    .padding(.horizontal, 16)
                    .padding(.top, 8)
                    .padding(.bottom, 32)
                }
            }
            .navigationTitle("Choose a book")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
            .task {
                if loadingRecents {
                    recents = (try? await appStore.safeCore.getRecentBooks(limit: 24)) ?? []
                    loadingRecents = false
                }
            }
            .task(id: query) {
                await runSearch()
            }
            .fullScreenCover(isPresented: $showScanner) {
                BookScannerView { isbn in
                    if let isbn {
                        beginResolve(isbn)
                    }
                }
            }
            .sheet(isPresented: $showManualEntry) {
                ManualISBNEntryView { isbn in
                    if let isbn {
                        beginResolve(isbn)
                    }
                }
                .presentationDetents([.medium])
            }
            .sheet(
                item: Binding<ResolvingSheet?>(
                    get: {
                        if let resolvingISBN { return ResolvingSheet(isbn: resolvingISBN) }
                        return nil
                    },
                    set: { newValue in
                        if newValue == nil {
                            resolvingISBN = nil
                            resolvedPreview = nil
                            resolveError = nil
                        }
                    }
                )
            ) { sheet in
                ISBNPreviewSheet(
                    isbn: sheet.isbn,
                    preview: resolvedPreview,
                    error: resolveError,
                    onUse: { preview in
                        selection = .pending(preview)
                        resolvingISBN = nil
                        dismiss()
                    },
                    onCancel: {
                        resolvingISBN = nil
                        resolvedPreview = nil
                        resolveError = nil
                    },
                    onEditTitle: { updated in
                        resolvedPreview = updated
                    }
                )
                .environment(appStore)
                .presentationDetents([.medium, .large])
            }
        }
    }

    // MARK: - Search + scan bar

    private var searchScanBar: some View {
        HStack(spacing: 10) {
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(Color.highlighterInkMuted)
                TextField("Search your books or paste ISBN", text: $query)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .focused($searchFocused)
                    .onSubmit(handleSubmit)
                if !query.isEmpty {
                    Button {
                        query = ""
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(Color.highlighterInkMuted.opacity(0.7))
                    }
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(Color.white.opacity(0.5), in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(Color.highlighterRule, lineWidth: 1))

            Button {
                showScanner = true
            } label: {
                Image(systemName: "barcode.viewfinder")
                    .font(.title3.weight(.medium))
                    .foregroundStyle(.white)
                    .frame(width: 44, height: 44)
                    .background(Color.highlighterAccent, in: RoundedRectangle(cornerRadius: 12))
            }
            .accessibilityLabel("Scan a barcode")
        }
    }

    // MARK: - Recents

    private var recentsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            if loadingRecents {
                HStack(spacing: 8) {
                    ProgressView().scaleEffect(0.8)
                    Text("Loading your books…")
                        .font(.footnote)
                        .foregroundStyle(Color.highlighterInkMuted)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            } else if recents.isEmpty {
                emptyRecentsCard
            } else {
                Text("Recent")
                    .font(.caption.weight(.medium))
                    .tracking(0.5)
                    .foregroundStyle(Color.highlighterInkMuted)
                coverGrid(recents)
            }
        }
    }

    private var emptyRecentsCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("No books yet")
                .font(.headline)
                .foregroundStyle(Color.highlighterInkStrong)
            Text("Scan a barcode or paste an ISBN to start your library.")
                .font(.footnote)
                .foregroundStyle(Color.highlighterInkMuted)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.highlighterRule.opacity(0.4), in: RoundedRectangle(cornerRadius: 12))
    }

    private func coverGrid(_ books: [ArtifactRecord]) -> some View {
        let columns = [
            GridItem(.flexible(), spacing: 12),
            GridItem(.flexible(), spacing: 12),
            GridItem(.flexible(), spacing: 12)
        ]
        return LazyVGrid(columns: columns, spacing: 16) {
            ForEach(books, id: \.shareEventId) { book in
                Button {
                    selection = .existing(book)
                    UIImpactFeedbackGenerator(style: .soft).impactOccurred()
                    dismiss()
                } label: {
                    coverCell(for: book)
                }
                .buttonStyle(.plain)
            }
        }
    }

    @ViewBuilder
    private func coverCell(for book: ArtifactRecord) -> some View {
        let image = book.preview.image
        VStack(alignment: .leading, spacing: 6) {
            Group {
                if !image.isEmpty, let url = URL(string: image) {
                    KFImage(url)
                        .placeholder { coverPlaceholder(title: book.preview.title) }
                        .fade(duration: 0.15)
                        .resizable()
                        .scaledToFill()
                } else {
                    coverPlaceholder(title: book.preview.title)
                }
            }
            .aspectRatio(2.0/3.0, contentMode: .fill)
            .frame(maxWidth: .infinity)
            .clipShape(RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6).stroke(Color.highlighterInkStrong.opacity(0.08), lineWidth: 1))
            .shadow(color: .black.opacity(0.08), radius: 4, x: 0, y: 2)

            if !book.preview.title.isEmpty {
                Text(book.preview.title)
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(2)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    private func coverPlaceholder(title: String) -> some View {
        ZStack {
            Color.highlighterRule.opacity(0.6)
            VStack(spacing: 6) {
                Image(systemName: "book.closed")
                    .font(.title2)
                    .foregroundStyle(Color.highlighterInkMuted)
                if !title.isEmpty {
                    Text(title)
                        .font(.caption2)
                        .multilineTextAlignment(.center)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .padding(.horizontal, 6)
                        .lineLimit(3)
                }
            }
        }
    }

    // MARK: - Search

    private var searchResultsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            if searching {
                HStack(spacing: 8) {
                    ProgressView().scaleEffect(0.8)
                    Text("Searching your rooms…")
                        .font(.footnote)
                        .foregroundStyle(Color.highlighterInkMuted)
                }
            } else if searchResults.isEmpty {
                noSearchHitsCard
            } else {
                Text("In your rooms")
                    .font(.caption.weight(.medium))
                    .tracking(0.5)
                    .foregroundStyle(Color.highlighterInkMuted)
                ForEach(searchResults, id: \.shareEventId) { book in
                    Button {
                        selection = .existing(book)
                        UIImpactFeedbackGenerator(style: .soft).impactOccurred()
                        dismiss()
                    } label: {
                        searchRow(book)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private var noSearchHitsCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("No matches in your rooms")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(Color.highlighterInkStrong)
            Text("If you know the ISBN, scan the back cover or paste it into the search field.")
                .font(.footnote)
                .foregroundStyle(Color.highlighterInkMuted)
            if let isbn = ISBNValidator.validate(query) {
                Button {
                    beginResolve(isbn)
                } label: {
                    Label("Look up ISBN", systemImage: "sparkle.magnifyingglass")
                        .font(.callout.weight(.semibold))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 10)
                        .background(Color.highlighterAccent, in: Capsule())
                }
                .padding(.top, 4)
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.highlighterRule.opacity(0.4), in: RoundedRectangle(cornerRadius: 12))
    }

    private func searchRow(_ book: ArtifactRecord) -> some View {
        HStack(spacing: 12) {
            if !book.preview.image.isEmpty, let url = URL(string: book.preview.image) {
                KFImage(url)
                    .placeholder { coverPlaceholder(title: book.preview.title) }
                    .fade(duration: 0.15)
                    .resizable()
                    .scaledToFill()
                    .frame(width: 42, height: 62)
                    .clipShape(RoundedRectangle(cornerRadius: 4))
            } else {
                coverPlaceholder(title: book.preview.title)
                    .frame(width: 42, height: 62)
                    .clipShape(RoundedRectangle(cornerRadius: 4))
            }
            VStack(alignment: .leading, spacing: 2) {
                Text(book.preview.title.isEmpty ? "Untitled" : book.preview.title)
                    .font(.body)
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(2)
                if !book.preview.author.isEmpty {
                    Text(book.preview.author)
                        .font(.caption)
                        .foregroundStyle(Color.highlighterInkMuted)
                }
            }
            Spacer()
            Image(systemName: "chevron.right")
                .font(.caption.weight(.medium))
                .foregroundStyle(Color.highlighterInkMuted)
        }
        .padding(12)
        .background(Color.white.opacity(0.5), in: RoundedRectangle(cornerRadius: 10))
    }

    // MARK: - Photo-only

    private var photoOnlyRow: some View {
        Button {
            selection = nil
            dismiss()
        } label: {
            HStack {
                Image(systemName: "photo")
                    .foregroundStyle(Color.highlighterInkMuted)
                Text("Share as photo only")
                    .foregroundStyle(Color.highlighterInkStrong)
                Spacer()
                if selection == nil {
                    Image(systemName: "checkmark")
                        .foregroundStyle(Color.highlighterAccent)
                }
            }
            .font(.callout)
            .padding(14)
            .background(Color.white.opacity(0.4), in: RoundedRectangle(cornerRadius: 12))
        }
        .buttonStyle(.plain)
    }

    // MARK: - Actions

    private func handleSubmit() {
        if let isbn = ISBNValidator.validate(query) {
            beginResolve(isbn)
        }
    }

    private func beginResolve(_ isbn: String) {
        // Dedup: if this ISBN already matches a book in the user's recents,
        // pick it directly and skip the catalog lookup + auto-publish. The
        // scan-for-already-known path is a discovery moment, not a form.
        let catalogId = "isbn:\(isbn)"
        if let existing = recents.first(where: { $0.preview.catalogId == catalogId }) {
            selection = .existing(existing)
            UINotificationFeedbackGenerator().notificationOccurred(.success)
            dismiss()
            return
        }

        resolvingISBN = isbn
        resolvedPreview = nil
        resolveError = nil
        Task {
            do {
                let preview = try await appStore.safeCore.lookupIsbn(isbn)
                // Only commit the preview if we're still on the same ISBN
                // (user could have cancelled mid-flight).
                if resolvingISBN == isbn {
                    resolvedPreview = preview
                }
            } catch {
                if resolvingISBN == isbn {
                    resolveError = error.localizedDescription
                }
            }
        }
    }

    private func runSearch() async {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            searchResults = []
            searching = false
            return
        }
        searching = true
        // Short debounce — we don't want to spam searches on every keystroke.
        try? await Task.sleep(nanoseconds: 180_000_000)
        guard query.trimmingCharacters(in: .whitespacesAndNewlines) == trimmed else { return }
        let results = (try? await appStore.safeCore.searchArtifacts(query: trimmed)) ?? []
        searchResults = results
        searching = false
    }
}
