import SwiftUI
import SwiftData

struct AddPodcastView: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.modelContext) private var modelContext
    @State private var feedURLString: String = ""
    @State private var isLoading = false
    @State private var errorMessage: String?

    private let podcastService = PodcastService()

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("RSS Feed URL", text: $feedURLString)
                        .textContentType(.URL)
                        .keyboardType(.URL)
                        .autocapitalization(.none)
                        .autocorrectionDisabled()
                } footer: {
                    Text("Enter the RSS feed URL of the podcast you want to subscribe to.")
                }

                if let error = errorMessage {
                    Section {
                        Text(error)
                            .foregroundStyle(.red)
                            .font(.footnote)
                    }
                }
            }
            .navigationTitle("Add Podcast")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                }

                ToolbarItem(placement: .confirmationAction) {
                    Button("Subscribe") {
                        subscribeToPodcast()
                    }
                    .disabled(feedURLString.isEmpty || isLoading)
                }
            }
            .overlay {
                if isLoading {
                    ProgressView()
                }
            }
        }
    }

    private func subscribeToPodcast() {
        guard let url = URL(string: feedURLString) else {
            errorMessage = "Invalid URL"
            return
        }

        isLoading = true
        errorMessage = nil

        Task {
            do {
                let podcast = try await podcastService.fetchFeed(url: url)

                await MainActor.run {
                    modelContext.insert(podcast)
                    isLoading = false
                    dismiss()
                }
            } catch {
                await MainActor.run {
                    errorMessage = error.localizedDescription
                    isLoading = false
                }
            }
        }
    }
}

#Preview {
    AddPodcastView()
        .modelContainer(for: [Podcast.self], inMemory: true)
}
