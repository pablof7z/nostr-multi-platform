import Foundation

struct BYOKCredentialImportResult: Identifiable, Sendable {
    var id: String { provider.rawValue }
    let provider: BYOKProvider
    let keyLabel: String?
}

enum PodcastBYOKCredentialImporter {
    static let providers = BYOKProvider.podcastPlayerDefaults

    static func apply(_ tokens: [BYOKProviderToken], to settings: inout Settings) throws -> [BYOKCredentialImportResult] {
        var imported: [BYOKCredentialImportResult] = []

        for token in tokens {
            guard let provider = BYOKProvider(rawValue: token.provider), !token.apiKey.isBlank else {
                continue
            }

            switch provider {
            case .openRouter:
                try OpenRouterCredentialStore.saveAPIKey(token.apiKey)
                settings.markOpenRouterBYOK(keyID: token.keyID, keyLabel: token.keyLabel)
            case .elevenLabs:
                try ElevenLabsCredentialStore.saveAPIKey(token.apiKey)
                settings.markElevenLabsBYOK(keyID: token.keyID, keyLabel: token.keyLabel)
            case .assemblyAI:
                try AssemblyAICredentialStore.saveAPIKey(token.apiKey)
            case .ollama:
                try OllamaCredentialStore.saveAPIKey(token.apiKey)
                settings.markOllamaBYOK(keyID: token.keyID, keyLabel: token.keyLabel)
            case .perplexity:
                try PerplexityCredentialStore.saveAPIKey(token.apiKey)
            }

            imported.append(BYOKCredentialImportResult(provider: provider, keyLabel: token.keyLabel))
        }

        return imported
    }

    static func hasStoredKey(for provider: BYOKProvider) -> Bool {
        switch provider {
        case .openRouter:
            OpenRouterCredentialStore.hasAPIKey()
        case .elevenLabs:
            ElevenLabsCredentialStore.hasAPIKey()
        case .assemblyAI:
            AssemblyAICredentialStore.hasAPIKey()
        case .ollama:
            OllamaCredentialStore.hasAPIKey()
        case .perplexity:
            PerplexityCredentialStore.hasAPIKey()
        }
    }
}
