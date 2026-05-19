import SwiftUI

struct SearchView: View {
    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    @State private var query = ""
    @FocusState private var fieldFocused: Bool

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                openEntityCard
                searchComingCard
            }
            .padding(.horizontal, 16)
            .padding(.top, 12)
        }
        .background(Color(.systemBackground))
        .navigationTitle("Search")
        .navigationBarTitleDisplayMode(.large)
        .onTapGesture { fieldFocused = false }
    }

    private var openEntityCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Open by ID")
                .font(.caption)
                .foregroundStyle(.secondary)

            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 8) {
                    Image(systemName: "number")
                        .foregroundStyle(hexValid ? Color.accentColor : .secondary)

                    TextField("64-character hex pubkey or event ID", text: $query)
                        .font(.footnote.monospaced())
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .focused($fieldFocused)
                        .submitLabel(.done)
                        .onSubmit { fieldFocused = false }

                    if !query.isEmpty {
                        Button {
                            query = ""
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                                .foregroundStyle(.secondary)
                        }
                    }
                }

                if !query.isEmpty && !hexValid {
                    HStack(spacing: 4) {
                        Image(systemName: "exclamationmark.circle")
                        Text("Must be a 64-character hex string (\(query.count)/64 chars)")
                    }
                    .font(.caption)
                    .foregroundStyle(.red)
                }

                Button {
                    guard hexValid else { return }
                    model.openAuthor(pubkey: query)
                    router.push(.profile(pubkey: query))
                    fieldFocused = false
                } label: {
                    Label("Open Profile", systemImage: "person.circle")
                }
                .disabled(!hexValid)

                Button {
                    guard hexValid else { return }
                    model.openThread(eventID: query)
                    router.push(.thread(eventID: query))
                    fieldFocused = false
                } label: {
                    Label("Open Thread", systemImage: "bubble.left.and.bubble.right")
                }
                .disabled(!hexValid)
            }
        }
    }

    private var searchComingCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Full-text Search")
                .font(.caption)
                .foregroundStyle(.secondary)

            HStack(spacing: 12) {
                Image(systemName: "magnifyingglass.circle")
                    .font(.system(size: 28))
                    .foregroundStyle(Color.accentColor)

                VStack(alignment: .leading, spacing: 4) {
                    Text("Full Search")
                        .font(.headline)
                    Text("Keyword and hashtag search across Nostr")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            VStack(alignment: .leading, spacing: 8) {
                featureLine(icon: "doc.text.magnifyingglass", label: "NIP-50 relay-backed full-text search")
                featureLine(icon: "number", label: "Hashtag discovery and trending topics")
                featureLine(icon: "person.2.wave.2", label: "People search by name or NIP-05")
            }
        }
    }

    private var hexValid: Bool {
        query.count == 64 && query.allSatisfy(\.isHexDigit)
    }

    private func featureLine(icon: String, label: String) -> some View {
        HStack(spacing: 8) {
            Image(systemName: icon)
                .foregroundStyle(Color.accentColor)
            Text(label)
                .font(.callout)
                .foregroundStyle(.secondary)
        }
    }
}
