import SwiftUI
import SwiftData

struct GuestAgentSheet: View {
    let guest: Guest
    let episode: Episode
    @Environment(\.dismiss) private var dismiss
    @Environment(\.modelContext) private var modelContext
    @State private var query: String = ""
    @State private var messages: [GuestChatMessage] = []
    @State private var isLoading = false

    private let aiService = AIService()
    private let ragService: RAGService

    init(guest: Guest, episode: Episode) {
        self.guest = guest
        self.episode = episode
        let vectorDB = VectorDatabase()
        let ai = AIService()
        self.ragService = RAGService(vectorDatabase: vectorDB, aiService: ai)
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                guestHeader

                if messages.isEmpty {
                    emptyState
                } else {
                    messageList
                }

                inputBar
            }
            .navigationTitle("Ask \(guest.name.components(separatedBy: " ").first ?? guest.name)")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        dismiss()
                    }
                }
            }
        }
    }

    private var guestHeader: some View {
        VStack(spacing: 8) {
            Image(systemName: "person.circle.fill")
                .font(.system(size: 60))
                .foregroundStyle(.secondary)

            Text(guest.name)
                .font(.headline)

            if let bio = guest.bio {
                Text(bio)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .lineLimit(3)
                    .padding(.horizontal)
            }
        }
        .padding()
        .background(Color(.systemGray6))
    }

    private var emptyState: some View {
        VStack(spacing: 16) {
            Spacer()

            Text("Ask \(guest.name.components(separatedBy: " ").first ?? guest.name) anything")
                .font(.headline)
                .multilineTextAlignment(.center)

            Text("This AI agent will respond based on what \(guest.name) has said in podcasts you've listened to.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)

            VStack(spacing: 8) {
                suggestionChip("What's your background?")
                suggestionChip("What advice would you give?")
                suggestionChip("What are you working on?")
            }

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
    }

    private var messageList: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 16) {
                    ForEach(messages) { message in
                        GuestMessageBubble(message: message, guestName: guest.name)
                            .id(message.id)
                    }

                    if isLoading {
                        HStack {
                            ProgressView()
                                .padding()
                            Spacer()
                        }
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
            TextField("Ask \(guest.name.components(separatedBy: " ").first ?? "")...", text: $query)
                .textFieldStyle(.plain)
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .background(Color(.systemGray6))
                .clipShape(Capsule())
                .disabled(isLoading)
                .accessibilityIdentifier("guestQueryInput")
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
            .accessibilityIdentifier("guestSendButton")
        }
        .padding()
        .background(Color(.systemBackground))
    }

    private func sendQuery() {
        let trimmedQuery = query.trimmingCharacters(in: .whitespaces)
        guard !trimmedQuery.isEmpty else { return }

        let userMessage = GuestChatMessage(role: .user, content: trimmedQuery)
        messages.append(userMessage)
        query = ""
        isLoading = true

        Task {
            do {
                let response = try await generateGuestResponse(to: trimmedQuery)

                await MainActor.run {
                    let assistantMessage = GuestChatMessage(role: .guest, content: response)
                    messages.append(assistantMessage)
                    isLoading = false
                }
            } catch {
                await MainActor.run {
                    let errorMessage = GuestChatMessage(
                        role: .guest,
                        content: "I'm sorry, I couldn't process that question. Please try again."
                    )
                    messages.append(errorMessage)
                    isLoading = false
                }
            }
        }
    }

    private func generateGuestResponse(to query: String) async throws -> String {
        // Build context from guest's appearances
        var context = "You are \(guest.name).\n\n"

        if let bio = guest.bio {
            context += "Your background: \(bio)\n\n"
        }

        // Get relevant transcript excerpts
        if let transcript = episode.transcript {
            let guestMentions = extractGuestContext(from: transcript.fullText)
            if !guestMentions.isEmpty {
                context += "Excerpts from your podcast appearance:\n\(guestMentions)\n\n"
            }
        }

        let systemPrompt = """
        You are \(guest.name), responding as if you were being interviewed.
        Base your responses on the context provided from podcast transcripts.
        Speak in first person and maintain a conversational, authentic tone.
        If asked about something not in the context, you can give a general response
        but indicate it's your general view, not something you specifically discussed.

        Context from your podcast appearances:
        \(context)
        """

        let history = messages.map { msg in
            AIChatMessage(
                role: msg.role == .user ? .user : .assistant,
                content: msg.content
            )
        }

        return try await aiService.chat(
            messages: history + [AIChatMessage(role: .user, content: query)],
            systemPrompt: systemPrompt
        )
    }

    private func extractGuestContext(from transcript: String) -> String {
        let sentences = transcript.components(separatedBy: CharacterSet(charactersIn: ".!?"))
        let relevantSentences = sentences.filter { sentence in
            sentence.localizedCaseInsensitiveContains(guest.name) ||
            sentence.localizedCaseInsensitiveContains(guest.name.components(separatedBy: " ").first ?? guest.name) ||
            sentence.contains("I ") || sentence.contains("I'm ") || sentence.contains("my ")
        }
        return relevantSentences.prefix(10).joined(separator: ". ")
    }
}

struct GuestChatMessage: Identifiable {
    let id = UUID()
    let role: Role
    let content: String

    enum Role {
        case user
        case guest
    }
}

struct GuestMessageBubble: View {
    let message: GuestChatMessage
    let guestName: String

    var body: some View {
        HStack {
            if message.role == .user { Spacer() }

            VStack(alignment: message.role == .user ? .trailing : .leading, spacing: 4) {
                if message.role == .guest {
                    Text(guestName.components(separatedBy: " ").first ?? guestName)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }

                Text(message.content)
                    .font(.subheadline)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .background(message.role == .user ? Color.accentColor : Color(.systemGray6))
                    .foregroundStyle(message.role == .user ? .white : .primary)
                    .clipShape(RoundedRectangle(cornerRadius: 18))
            }
            .frame(maxWidth: 300, alignment: message.role == .user ? .trailing : .leading)

            if message.role == .guest { Spacer() }
        }
    }
}

#Preview {
    let guest = Guest(name: "Naval Ravikant", bio: "Entrepreneur, investor, and philosopher.")
    let podcast = Podcast(feedURL: URL(string: "https://example.com")!, title: "Test", author: "Test")
    let episode = Episode(podcast: podcast, guid: "123", title: "Test Episode", audioURL: URL(string: "https://example.com/audio.mp3")!)

    GuestAgentSheet(guest: guest, episode: episode)
}
