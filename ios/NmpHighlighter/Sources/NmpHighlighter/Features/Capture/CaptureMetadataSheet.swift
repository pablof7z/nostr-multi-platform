import Kingfisher
import SwiftUI

/// Destination + metadata sheet shown after the user taps "Next →" on the
/// capture canvas. Collects book, room, and note before publishing.
struct CaptureMetadataSheet: View {
    @Bindable var store: CaptureStore
    let onPublish: () -> Void

    @Environment(HighlighterStore.self) private var appStore
    @Environment(\.dismiss) private var dismiss

    @State private var showBookPicker = false
    @State private var showCommunityPicker = false

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 12) {
                    // Upload / error status
                    if store.isUploading {
                        HStack(spacing: 6) {
                            ProgressView().scaleEffect(0.7).tint(Color.highlighterInkMuted)
                            Text("Uploading…")
                                .font(.caption)
                                .foregroundStyle(Color.highlighterInkMuted)
                            Spacer()
                        }
                        .padding(.horizontal, 16)
                    } else if let err = store.uploadError {
                        HStack(spacing: 6) {
                            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.red)
                            Text(err).font(.caption).lineLimit(1)
                            Spacer()
                            Button("Retry") { store.retryUpload() }
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(Color.highlighterAccent)
                        }
                        .padding(.horizontal, 16)
                    }

                    bookPill
                    communityRow

                    TextField("Add a note (optional)", text: $store.note, axis: .vertical)
                        .lineLimit(1...4)
                        .font(.callout)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 10)
                        .background(Color.highlighterPaper, in: RoundedRectangle(cornerRadius: 12))
                        .overlay(RoundedRectangle(cornerRadius: 12).stroke(Color.highlighterRule, lineWidth: 1))
                        .padding(.horizontal, 16)

                    publishButton
                        .padding(.horizontal, 16)
                        .padding(.top, 4)
                }
                .padding(.top, 16)
                .padding(.bottom, 32)
            }
            .background(Color.highlighterPaper.ignoresSafeArea())
            .navigationTitle(store.stashedQuote != nil ? "Publish Highlight" : "Share Photo")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
        .sheet(isPresented: $showBookPicker) {
            BookPicker(selection: $store.selectedBook).environment(appStore)
        }
        .sheet(isPresented: $showCommunityPicker) {
            CommunityPicker(selection: $store.selectedGroupId).environment(appStore)
        }
    }

    // MARK: - Book pill

    @ViewBuilder
    private var bookPill: some View {
        Button { showBookPicker = true } label: {
            HStack(spacing: 10) {
                bookCover
                bookText
                Spacer(minLength: 0)
                Image(systemName: "chevron.right")
                    .font(.caption.weight(.medium))
                    .foregroundStyle(Color.highlighterInkMuted)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(Color.highlighterPaper, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(Color.highlighterRule, lineWidth: 1))
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 16)
    }

    @ViewBuilder
    private var bookCover: some View {
        if let sel = store.selectedBook, !sel.coverURL.isEmpty, let url = URL(string: sel.coverURL) {
            KFImage(url)
                .placeholder { bookCoverPlaceholder }
                .fade(duration: 0.15)
                .resizable()
                .scaledToFill()
                .frame(width: 30, height: 42)
                .clipShape(RoundedRectangle(cornerRadius: 3))
        } else if store.selectedBook != nil {
            bookCoverPlaceholder
        } else {
            Image(systemName: "book.closed")
                .font(.body)
                .foregroundStyle(Color.highlighterInkMuted)
                .frame(width: 30, height: 42)
        }
    }

    private var bookCoverPlaceholder: some View {
        ZStack {
            Color.highlighterRule.opacity(0.5)
            Image(systemName: "book.closed").foregroundStyle(Color.highlighterInkMuted)
        }
        .frame(width: 30, height: 42)
        .clipShape(RoundedRectangle(cornerRadius: 3))
    }

    @ViewBuilder
    private var bookText: some View {
        if let sel = store.selectedBook {
            VStack(alignment: .leading, spacing: 2) {
                Text(sel.title.isEmpty ? "Untitled" : sel.title)
                    .font(.callout.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkStrong)
                    .lineLimit(1)
                if !sel.author.isEmpty {
                    Text(sel.author)
                        .font(.caption)
                        .foregroundStyle(Color.highlighterInkMuted)
                        .lineLimit(1)
                }
            }
        } else {
            VStack(alignment: .leading, spacing: 2) {
                Text("Add a book")
                    .font(.callout.weight(.semibold))
                    .foregroundStyle(Color.highlighterInkStrong)
                Text("Optional — scan barcode or search")
                    .font(.caption)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .lineLimit(1)
            }
        }
    }

    // MARK: - Community row

    private var communityRow: some View {
        Button { showCommunityPicker = true } label: {
            HStack(spacing: 8) {
                Image(systemName: "number")
                    .font(.caption)
                    .foregroundStyle(Color.highlighterInkMuted)
                    .frame(width: 18)
                Text("Room")
                    .font(.callout)
                    .foregroundStyle(Color.highlighterInkStrong)
                Spacer()
                Text(communityName.isEmpty ? "Optional" : communityName)
                    .font(.callout)
                    .foregroundStyle(communityName.isEmpty ? Color.highlighterInkMuted : Color.highlighterAccent)
                    .lineLimit(1)
                Image(systemName: "chevron.right")
                    .font(.caption.weight(.medium))
                    .foregroundStyle(Color.highlighterInkMuted)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(Color.highlighterPaper, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(Color.highlighterRule, lineWidth: 1))
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 16)
    }

    private var communityName: String {
        guard let id = store.selectedGroupId else { return "" }
        return appStore.joinedCommunities.first(where: { $0.id == id })?.name ?? id
    }

    // MARK: - Publish button

    private var publishButton: some View {
        Button {
            onPublish()
        } label: {
            HStack(spacing: 8) {
                Image(systemName: store.stashedQuote != nil ? "highlighter" : "photo")
                Text(publishLabel).fontWeight(.semibold)
                Spacer()
                Image(systemName: "arrow.up")
            }
            .font(.body)
            .foregroundStyle(.white)
            .padding(.horizontal, 18)
            .padding(.vertical, 14)
            .background(
                store.canPublish ? Color.highlighterAccent : Color.highlighterInkMuted.opacity(0.4),
                in: RoundedRectangle(cornerRadius: 14)
            )
        }
        .disabled(!store.canPublish)
    }

    private var publishLabel: String {
        if let q = store.stashedQuote, !q.isEmpty {
            return store.selectedBook != nil ? "Publish highlight" : "Pick a book first"
        }
        return "Share photo"
    }
}
