import Foundation
import os.log

enum ElevenLabsCredentialStore {
    private static let logger = Logger.app("ElevenLabsCredentialStore")
    private static let service = "\(Bundle.main.bundleIdentifier ?? "Podcastr").elevenlabs"
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
            logger.error("ElevenLabsCredentialStore.hasAPIKey failed: \(error, privacy: .public)")
            return false
        }
    }

    static func deleteAPIKey() throws {
        try KeychainStore.deleteString(service: service, account: account)
    }
}
