import SwiftUI

struct SearchView: View {
    @EnvironmentObject private var model: KernelModel
    @EnvironmentObject private var router: ChirpRouter

    @State private var query = ""
    @FocusState private var fieldFocused: Bool

    var body: some View {
        Form {
            openEntitySection
        }
        .scrollContentBackground(.hidden)
        .chirpScreenBackground()
        .navigationTitle("Search")
        .navigationBarTitleDisplayMode(.large)
        .onTapGesture { fieldFocused = false }
    }

    private var openEntitySection: some View {
        // §4.4: validity of a 64-character hex string is protocol logic; the
        // iOS shell dispatches unconditionally. `openAuthor` / `openThread`
        // are fire-and-forget, and the kernel surfaces a toast through the
        // existing `lastErrorToast` projection if `query` is malformed.
        Section("Open by ID") {
            HStack(spacing: 8) {
                Image(systemName: "number")
                    .foregroundStyle(.secondary)

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

            HStack {
                Button {
                    model.openAuthor(pubkey: query)
                    router.push(.profile(pubkey: query))
                    fieldFocused = false
                } label: {
                    Label("Open Profile", systemImage: "person.circle")
                }
                .disabled(query.isEmpty)

                Spacer()

                Button {
                    model.openThread(eventID: query)
                    router.push(.thread(eventID: query))
                    fieldFocused = false
                } label: {
                    Label("Open Thread", systemImage: "bubble.left.and.bubble.right")
                }
                .disabled(query.isEmpty)
            }
        }
    }
}
