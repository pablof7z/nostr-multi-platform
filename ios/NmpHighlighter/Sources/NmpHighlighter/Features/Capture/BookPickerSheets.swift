import Kingfisher
import SwiftUI

struct ResolvingSheet: Identifiable, Equatable {
    let isbn: String
    var id: String { isbn }
}

/// Shown after a successful scan or manual entry. Resolves the ISBN via
/// Open Library while the user looks at a placeholder, then crossfades the
/// cover in. If the catalog returns nothing, the user can still type a title
/// inline — the ISBN alone is a valid NIP-73 reference.
struct ISBNPreviewSheet: View {
    let isbn: String
    let preview: ArtifactPreview?
    let error: String?
    var onUse: (ArtifactPreview) -> Void
    var onCancel: () -> Void
    var onEditTitle: (ArtifactPreview) -> Void

    @Environment(\.dismiss) private var dismiss
    @Environment(HighlighterStore.self) private var appStore
    @State private var manualTitle: String = ""
    @State private var manualAuthor: String = ""

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 20) {
                    coverArea
                    textFields
                    Spacer(minLength: 0)
                }
                .padding(20)
            }
            .background(Color.highlighterPaper.ignoresSafeArea())
            .navigationTitle("Is this right?")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        onCancel()
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Use") { commit() }
                        .fontWeight(.semibold)
                        .disabled(effectiveTitle.isEmpty)
                }
            }
            .onAppear {
                manualTitle = preview?.title ?? ""
                manualAuthor = preview?.author ?? ""
            }
            .onChange(of: preview?.title) { _, newTitle in
                if manualTitle.isEmpty, let newTitle, !newTitle.isEmpty {
                    manualTitle = newTitle
                }
            }
            .onChange(of: preview?.author) { _, newAuthor in
                if manualAuthor.isEmpty, let newAuthor, !newAuthor.isEmpty {
                    manualAuthor = newAuthor
                }
            }
        }
    }

    private var effectiveTitle: String {
        manualTitle.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    @ViewBuilder
    private var coverArea: some View {
        ZStack {
            if let image = preview?.image, !image.isEmpty, let url = URL(string: image) {
                KFImage(url)
                    .placeholder { coverPlaceholder }
                    .fade(duration: 0.25)
                    .resizable()
                    .scaledToFit()
                    .frame(height: 200)
            } else {
                coverPlaceholder
                    .frame(height: 200)
            }
        }
        .animation(.easeOut(duration: 0.25), value: preview?.image)
    }

    private var coverPlaceholder: some View {
        VStack(spacing: 8) {
            if preview == nil, error == nil {
                ProgressView()
                Text("Looking up…")
                    .font(.footnote)
                    .foregroundStyle(Color.highlighterInkMuted)
            } else {
                Image(systemName: "book.closed")
                    .font(.system(size: 36, weight: .light))
                    .foregroundStyle(Color.highlighterInkMuted)
                Text(isbn)
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(Color.highlighterInkMuted)
            }
        }
        .frame(maxWidth: .infinity, minHeight: 200)
        .background(Color.highlighterRule.opacity(0.5), in: RoundedRectangle(cornerRadius: 12))
    }

    private var textFields: some View {
        VStack(alignment: .leading, spacing: 16) {
            if let error, preview == nil {
                Label(error, systemImage: "exclamationmark.triangle")
                    .font(.footnote)
                    .foregroundStyle(.red)
            }

            VStack(alignment: .leading, spacing: 6) {
                Text("TITLE")
                    .font(.caption2.weight(.semibold))
                    .tracking(0.5)
                    .foregroundStyle(Color.highlighterInkMuted)
                TextField("Book title", text: $manualTitle)
                    .font(.system(.body, design: .default))
                    .padding(12)
                    .background(Color.white.opacity(0.5), in: RoundedRectangle(cornerRadius: 10))
                    .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.highlighterRule, lineWidth: 1))
            }

            VStack(alignment: .leading, spacing: 6) {
                Text("AUTHOR")
                    .font(.caption2.weight(.semibold))
                    .tracking(0.5)
                    .foregroundStyle(Color.highlighterInkMuted)
                TextField("Author (optional)", text: $manualAuthor)
                    .font(.body)
                    .padding(12)
                    .background(Color.white.opacity(0.5), in: RoundedRectangle(cornerRadius: 10))
                    .overlay(RoundedRectangle(cornerRadius: 10).stroke(Color.highlighterRule, lineWidth: 1))
            }

            HStack(spacing: 6) {
                Image(systemName: "barcode")
                Text(isbn)
                    .monospacedDigit()
            }
            .font(.caption)
            .foregroundStyle(Color.highlighterInkMuted)
        }
    }

    private func commit() {
        guard !effectiveTitle.isEmpty else { return }
        let base = preview
        // Always derive reference/highlight tags from the ISBN we scanned —
        // the catalog API may return these empty or wrong.
        let catalogId = "isbn:\(isbn)"
        let updated = ArtifactPreview(
            id: base?.id ?? "",
            url: base?.url ?? "",
            title: effectiveTitle,
            author: manualAuthor.trimmingCharacters(in: .whitespacesAndNewlines),
            image: base?.image ?? "",
            description: base?.description ?? "",
            source: "book",
            domain: base?.domain ?? "",
            catalogId: catalogId,
            catalogKind: "isbn",
            podcastGuid: "",
            podcastItemGuid: "",
            podcastShowTitle: "",
            audioUrl: "",
            audioPreviewUrl: "",
            transcriptUrl: "",
            feedUrl: "",
            publishedAt: base?.publishedAt ?? "",
            durationSeconds: nil,
            referenceTagName: "i",
            referenceTagValue: catalogId,
            referenceKind: "isbn",
            highlightTagName: "i",
            highlightTagValue: catalogId,
            highlightReferenceKey: "i:\(catalogId)",
            chapters: []
        )
        onEditTitle(updated)
        onUse(updated)
        dismiss()
    }
}
