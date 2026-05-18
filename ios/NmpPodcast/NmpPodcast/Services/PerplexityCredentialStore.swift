import Foundation
import os.log

/// Typed credential store for the Perplexity online-search API key.
///
/// Mirrors the API of `OpenRouterCredentialStore` and
/// `ElevenLabsCredentialStore` so the three keys can be managed
/// uniformly by Settings views, the agent, and the data-export redactor.
///
/// **Service / account constants** are deliberately the same literals
/// `PerplexityClient` was already reading via raw `KeychainStore` — so any
/// key already saved by the agent's first online search reads back through
/// the new store without migration.
enum PerplexityCredentialStore {

    private static let logger = Logger.app("PerplexityCredentialStore")

    /// Keychain service identifier. Matches the literal `PerplexityClient`
    /// has been using since Lane 10 so existing keys stay reachable.
    static let service: String = "PerplexityCredentialStore"

    /// Keychain account identifier.
    static let account: String = "perplexity_api_key"

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
            logger.error("PerplexityCredentialStore.hasAPIKey failed: \(error, privacy: .public)")
            return false
        }
    }

    static func deleteAPIKey() throws {
        try KeychainStore.deleteString(service: service, account: account)
    }
}
