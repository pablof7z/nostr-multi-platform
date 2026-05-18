import Foundation
import os.log

enum OllamaCredentialStore {
    private static let logger = Logger.app("OllamaCredentialStore")
    private static let service = "\(Bundle.main.bundleIdentifier ?? "Podcastr").ollama"
    private static let account = "api-key"

    static func saveAPIKey(_ apiKey: String) throws {
        let trimmed = apiKey.trimmed
        guard !trimmed.isEmpty else { return }
        try KeychainStore.saveString(trimmed, service: service, account: account)
    }

    static func apiKey() throws -> String? {
        guard let value = try KeychainStore.readString(service: service, account: account) else {
            return nil
        }
        let trimmed = value.trimmed
        return trimmed.isEmpty ? nil : trimmed
    }

    static func hasAPIKey() -> Bool {
        do {
            return try apiKey() != nil
        } catch {
            logger.error("OllamaCredentialStore.hasAPIKey failed: \(error, privacy: .public)")
            return false
        }
    }

    static func deleteAPIKey() throws {
        try KeychainStore.deleteString(service: service, account: account)
    }
}

enum LLMProviderCredentialResolver {
    static func apiKey(for provider: LLMProvider) throws -> String? {
        switch provider {
        case .openRouter:
            return try OpenRouterCredentialStore.apiKey()
        case .ollama:
            return try OllamaCredentialStore.apiKey()
        }
    }

    static func hasAPIKey(for provider: LLMProvider) -> Bool {
        switch provider {
        case .openRouter:
            return OpenRouterCredentialStore.hasAPIKey()
        case .ollama:
            return OllamaCredentialStore.hasAPIKey()
        }
    }

    static func missingCredentialMessage(for provider: LLMProvider) -> String {
        "\(provider.displayName) is not connected. Add a key in Settings."
    }
}
