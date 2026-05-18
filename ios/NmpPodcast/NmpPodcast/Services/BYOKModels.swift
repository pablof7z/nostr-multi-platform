import Foundation

// MARK: - Authorization & token DTOs

enum BYOKProvider: String, CaseIterable, Identifiable, Sendable {
    case openRouter = "openrouter"
    case elevenLabs = "elevenlabs"
    case assemblyAI = "assemblyai"
    case ollama = "ollama"
    case perplexity = "perplexity"

    var id: String { rawValue }
    var scope: String { "key:\(rawValue)" }

    var displayName: String {
        switch self {
        case .openRouter:  return "OpenRouter"
        case .elevenLabs:  return "ElevenLabs"
        case .assemblyAI:  return "AssemblyAI"
        case .ollama:      return "Ollama Cloud"
        case .perplexity:  return "Perplexity"
        }
    }

    var iconName: String {
        switch self {
        case .openRouter:  return "key.viewfinder"
        case .elevenLabs:  return "waveform"
        case .assemblyAI:  return "waveform.badge.mic"
        case .ollama:      return "cloud.fill"
        case .perplexity:  return "magnifyingglass.circle.fill"
        }
    }

    static let podcastPlayerDefaults: [BYOKProvider] = [
        .openRouter,
        .elevenLabs,
        .assemblyAI,
        .ollama,
        .perplexity,
    ]
}

/// Single in-flight authorization. The `state` and `codeVerifier` must
/// survive across the web-auth callback for PKCE + CSRF validation.
struct BYOKPendingAuthorization {
    let provider: String
    let authorizationURL: URL
    let redirectURI: String
    let clientID: String
    let state: String
    let codeVerifier: String
}

struct BYOKTokenRequest: Encodable {
    let grantType = "authorization_code"
    let code: String
    let codeVerifier: String
    let clientID: String
    let redirectURI: String

    private enum CodingKeys: String, CodingKey {
        case grantType = "grant_type"
        case code
        case codeVerifier = "code_verifier"
        case clientID = "client_id"
        case redirectURI = "redirect_uri"
    }
}

struct BYOKProviderToken: Decodable, Sendable, Identifiable {
    var id: String { provider }
    let provider: String
    let apiKey: String
    let keyID: String?
    let keyLabel: String?

    private enum CodingKeys: String, CodingKey {
        case provider
        case apiKey = "api_key"
        case keyID = "key_id"
        case keyLabel = "key_label"
    }

    init(provider: String, apiKey: String, keyID: String?, keyLabel: String?) {
        self.provider = provider.lowercased()
        self.apiKey = apiKey
        self.keyID = keyID
        self.keyLabel = keyLabel
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        provider = try container.decode(String.self, forKey: .provider).lowercased()
        apiKey = try container.decode(String.self, forKey: .apiKey)
        keyID = try container.decodeIfPresent(String.self, forKey: .keyID)
        keyLabel = try container.decodeIfPresent(String.self, forKey: .keyLabel)
    }
}

struct BYOKTokenResponse: Decodable, Sendable {
    let tokenType: String
    let provider: String
    let apiKey: String
    let keyID: String?
    let keyLabel: String?
    let appName: String?
    let issuedAt: Int?
    let providers: [BYOKProviderToken]

    private enum CodingKeys: String, CodingKey {
        case tokenType = "token_type"
        case provider
        case apiKey = "api_key"
        case keyID = "key_id"
        case keyLabel = "key_label"
        case appName = "app_name"
        case issuedAt = "issued_at"
        case providers
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        tokenType = try container.decode(String.self, forKey: .tokenType)
        appName = try container.decodeIfPresent(String.self, forKey: .appName)
        issuedAt = try container.decodeIfPresent(Int.self, forKey: .issuedAt)

        let decodedProviders = try container.decodeIfPresent([BYOKProviderToken].self, forKey: .providers) ?? []
        if let firstProvider = decodedProviders.first {
            providers = decodedProviders
            provider = firstProvider.provider
            apiKey = firstProvider.apiKey
            keyID = firstProvider.keyID
            keyLabel = firstProvider.keyLabel
        } else {
            provider = try container.decode(String.self, forKey: .provider).lowercased()
            apiKey = try container.decode(String.self, forKey: .apiKey)
            keyID = try container.decodeIfPresent(String.self, forKey: .keyID)
            keyLabel = try container.decodeIfPresent(String.self, forKey: .keyLabel)
            providers = [
                BYOKProviderToken(
                    provider: provider,
                    apiKey: apiKey,
                    keyID: keyID,
                    keyLabel: keyLabel
                )
            ]
        }
    }
}

struct BYOKTokenErrorResponse: Decodable {
    let error: String?
}

// MARK: - Errors

enum BYOKConnectError: LocalizedError {
    case accessDenied
    case authenticationFailed
    case cancelled
    case invalidAuthorizationURL
    case invalidCallback
    case invalidTokenResponse
    case missingCode
    case noProviderKeysReturned
    case randomGenerationFailed
    case serverRejectedToken(error: String?)
    case stateMismatch
    case tokenExchangeFailed
    case unexpectedProvider

    var errorDescription: String? {
        switch self {
        case .accessDenied:
            "Access was denied in BYOK."
        case .authenticationFailed:
            "BYOK authentication could not be completed."
        case .cancelled:
            "BYOK connection was cancelled."
        case .invalidAuthorizationURL:
            "BYOK authorization URL could not be created."
        case .invalidCallback:
            "BYOK returned an unexpected callback."
        case .invalidTokenResponse:
            "BYOK returned an invalid token response."
        case .missingCode:
            "BYOK did not return an authorization code."
        case .noProviderKeysReturned:
            "BYOK did not return any selected provider keys."
        case .randomGenerationFailed:
            "Secure random generation failed."
        case .serverRejectedToken(let error):
            if let error, !error.isEmpty {
                "BYOK rejected the token exchange: \(error)"
            } else {
                "BYOK rejected the token exchange."
            }
        case .stateMismatch:
            "BYOK returned an invalid state."
        case .tokenExchangeFailed:
            "BYOK token exchange failed."
        case .unexpectedProvider:
            "BYOK returned a credential for the wrong provider."
        }
    }
}

// MARK: - Helpers

extension Data {
    /// RFC 4648 §5 base64url encoding (no padding) — required by OAuth PKCE.
    func base64URLEncodedString() -> String {
        base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }
}
