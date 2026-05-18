import SwiftUI
import SwiftData
import OSLog

struct AskView: View {
    @Environment(\.modelContext) private var modelContext
    @Query private var episodes: [Episode]
    @State private var query: String = ""
    @State private var messages: [ChatMessage] = []
    @State private var isLoading = false

    private let ragService = ServiceContainer.shared.ragService

    private var episodeCount: Int {
        episodes.filter { $0.hasBeenPlayed || $0.playbackPosition > 0 }.count
    }

    private var hoursListened: Double {
        episodes.reduce(0) { $0 + min($1.playbackPosition, $1.duration) } / 3600
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                if messages.isEmpty {
                    emptyState
                } else {
                    messageList
                }

                inputBar
            }
            .navigationTitle("Ask Your Podcasts")
            .navigationBarTitleDisplayMode(.inline)
        }
        .task {
            do {
                try await ragService.vectorDatabase.open()
            } catch {
                Logger.database.error("Failed to open vector database: \(error)")
            }
        }
    }

    private var emptyState: some View {
        VStack(spacing: 16) {
            Spacer()

            Text("Ask Your Podcasts")
                .font(.title2)
                .fontWeight(.semibold)

            Text("\(episodeCount) episodes \u{2022} \(Int(hoursListened)) hours listened")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            VStack(alignment: .leading, spacing: 12) {
                Text("Ask me anything about the podcasts you've listened to.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)

                suggestionChip("Which episode discussed luck?")
                suggestionChip("Who talked about sleep?")
                suggestionChip("Explain proof of work")
            }
            .padding()
            .background(Color(.systemGray6))
            .clipShape(RoundedRectangle(cornerRadius: 16))
            .padding(.horizontal)

            Spacer()
        }
    }

    private func suggestionChip(_ text: String) -> some View {
        Button {
            query = text
            sendQuery()
        } label: {
            Text(text)
                .font(.footnote)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(Color(.systemBackground))
                .clipShape(Capsule())
                .overlay(Capsule().stroke(Color(.separator), lineWidth: 1))
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("suggestionChip")
    }

    private var messageList: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 16) {
                    ForEach(messages) { message in
                        MessageBubble(message: message)
                            .id(message.id)
                    }

                    if isLoading {
                        HStack {
                            ProgressView()
                                .padding()
                            Spacer()
                        }
                        .id("loading")
                    }
                }
                .padding()
            }
            .onChange(of: messages.count) { _, _ in
                if let lastMessage = messages.last {
                    withAnimation {
                        proxy.scrollTo(lastMessage.id, anchor: .bottom)
                    }
                }
            }
        }
    }

    private var inputBar: some View {
        HStack(spacing: 12) {
            TextField("Ask your podcasts...", text: $query)
                .textFieldStyle(.plain)
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .background(Color(.systemGray6))
                .clipShape(Capsule())
                .disabled(isLoading)
                .accessibilityIdentifier("askInput")
                .onSubmit {
                    sendQuery()
                }

            Button {
                sendQuery()
            } label: {
                Image(systemName: "arrow.up.circle.fill")
                    .font(.title)
                    .foregroundStyle(query.isEmpty || isLoading ? .gray : Color.accentColor)
            }
            .disabled(query.isEmpty || isLoading)
            .accessibilityIdentifier("sendButton")
        }
        .padding()
        .background(Color(.systemBackground))
    }

    private func sendQuery() {
        let trimmedQuery = query.trimmingCharacters(in: .whitespaces)
        guard !trimmedQuery.isEmpty else { return }

        let userMessage = ChatMessage(role: .user, content: trimmedQuery)
        messages.append(userMessage)
        query = ""
        isLoading = true

        Task {
            do {
                let history = messages.dropLast().map { msg in
                    AIChatMessage(
                        role: msg.role == .user ? .user : .assistant,
                        content: msg.content
                    )
                }

                let (response, sources) = try await ragService.chat(
                    query: trimmedQuery,
                    modelContext: modelContext,
                    history: Array(history)
                )

                let sourceRefs = sources.map { result in
                    SourceReference(
                        episodeTitle: result.episodeTitle ?? "Episode",
                        podcastTitle: result.podcastTitle ?? "Podcast",
                        timestamp: result.timestamp,
                        isInsight: result.isInsight
                    )
                }

                let assistantMessage = ChatMessage(
                    role: .assistant,
                    content: response,
                    sources: sourceRefs
                )

                await MainActor.run {
                    messages.append(assistantMessage)
                    isLoading = false
                }
            } catch {
                let errorText: String
                if let aiError = error as? AIServiceError {
                    errorText = aiError.errorDescription ?? "An unknown error occurred"
                } else if let localizedError = error as? LocalizedError {
                    errorText = localizedError.errorDescription ?? error.localizedDescription
                } else {
                    errorText = error.localizedDescription
                }

                Logger.ai.error("Ask query failed: \(error)")

                await MainActor.run {
                    let errorMessage = ChatMessage(
                        role: .assistant,
                        content: errorText
                    )
                    messages.append(errorMessage)
                    isLoading = false
                }
            }
        }
    }
}

struct ChatMessage: Identifiable {
    let id = UUID()
    let role: Role
    let content: String
    var sources: [SourceReference] = []

    enum Role {
        case user
        case assistant
    }
}

struct SourceReference: Identifiable {
    let id = UUID()
    let episodeTitle: String
    let podcastTitle: String
    let timestamp: TimeInterval?
    let isInsight: Bool
}

struct MessageBubble: View {
    let message: ChatMessage

    var body: some View {
        HStack {
            if message.role == .user { Spacer() }

            VStack(alignment: message.role == .user ? .trailing : .leading, spacing: 8) {
                Text(message.content)
                    .font(.subheadline)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .background(message.role == .user ? Color.accentColor : Color(.systemGray6))
                    .foregroundStyle(message.role == .user ? .white : .primary)
                    .clipShape(RoundedRectangle(cornerRadius: 18))

                if !message.sources.isEmpty {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Sources")
                            .font(.caption2)
                            .foregroundStyle(.secondary)

                        ForEach(message.sources) { source in
                            SourceChip(source: source)
                        }
                    }
                }
            }
            .frame(maxWidth: 300, alignment: message.role == .user ? .trailing : .leading)

            if message.role == .assistant { Spacer() }
        }
    }
}

struct SourceChip: View {
    let source: SourceReference

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: source.isInsight ? "lightbulb.fill" : "play.circle.fill")
                .font(.caption)
                .foregroundStyle(source.isInsight ? .yellow : .primary)

            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 4) {
                    Text(source.podcastTitle)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                    if source.isInsight {
                        Text("Your insight")
                            .font(.caption2)
                            .fontWeight(.medium)
                            .foregroundStyle(.yellow)
                    }
                }
                Text(source.episodeTitle)
                    .font(.caption)
                    .lineLimit(1)
            }

            if let timestamp = source.timestamp {
                Text(formatTime(timestamp))
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(source.isInsight ? Color.yellow.opacity(0.15) : Color(.systemGray5))
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .accessibilityIdentifier("sourceChip")
    }

    private func formatTime(_ time: TimeInterval) -> String {
        let minutes = Int(time) / 60
        let seconds = Int(time) % 60
        return String(format: "%d:%02d", minutes, seconds)
    }
}

#Preview {
    AskView()
        .modelContainer(for: [Episode.self], inMemory: true)
}
