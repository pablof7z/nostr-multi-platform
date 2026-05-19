import AuthenticationServices
import CryptoKit
import Foundation
import os.log
import Security
import UIKit

@MainActor
final class BYOKConnectService: NSObject, ASWebAuthenticationPresentationContextProviding {
    private let logger = Logger.app("BYOKConnectService")
    private static let encoder = JSONEncoder()
    private static let decoder = JSONDecoder()

    private enum Constants {
        static let stateByteCount: Int = 32
        static let codeVerifierByteCount: Int = 64
        static let tokenRequestTimeout: TimeInterval = 60
    }

    private let authorizationBaseURL = URL(string: "https://byok.f7z.io/authorize")!
    private let tokenURL = URL(string: "https://byok.f7z.io/api/token")!
    private let redirectScheme = "podcastr"
    private let redirectHost = "byok"
    private var currentSession: ASWebAuthenticationSession?

    func connectPodcastProviders() async throws -> [BYOKProviderToken] {
        try await connectProviders(BYOKProvider.podcastPlayerDefaults)
    }

    func connectProviders(_ providers: [BYOKProvider]) async throws -> [BYOKProviderToken] {
        var seenProviders = Set<String>()
        let uniqueProviders = providers.filter { seenProviders.insert($0.rawValue).inserted }
        let pending = try makeAuthorization(providers: uniqueProviders)
        let callbackURL = try await authenticate(url: pending.authorizationURL)
        let code = try authorizationCode(from: callbackURL, expectedState: pending.state)
        let response = try await exchangeCode(code, pending: pending)
        let requested = Set(uniqueProviders.map(\.rawValue))
        let returned = response.providers.filter { requested.contains($0.provider) && !$0.apiKey.isEmpty }
        guard !returned.isEmpty else {
            throw BYOKConnectError.noProviderKeysReturned
        }
        return returned
    }

    func connectOpenRouter() async throws -> BYOKTokenResponse {
        let pending = try makeAuthorization(provider: "openrouter", scope: "key:openrouter")
        let callbackURL = try await authenticate(url: pending.authorizationURL)
        let code = try authorizationCode(from: callbackURL, expectedState: pending.state)
        let token = try await exchangeCode(code, pending: pending)

        guard token.provider == "openrouter" else {
            throw BYOKConnectError.unexpectedProvider
        }
        guard token.tokenType == "raw_api_key", !token.apiKey.isEmpty else {
            throw BYOKConnectError.invalidTokenResponse
        }

        return token
    }

    func connectElevenLabs() async throws -> BYOKTokenResponse {
        let pending = try makeAuthorization(provider: "elevenlabs", scope: "key:elevenlabs")
        let callbackURL = try await authenticate(url: pending.authorizationURL)
        let code = try authorizationCode(from: callbackURL, expectedState: pending.state)
        let token = try await exchangeCode(code, pending: pending)

        guard token.provider == "elevenlabs" else {
            throw BYOKConnectError.unexpectedProvider
        }
        guard token.tokenType == "raw_api_key", !token.apiKey.isEmpty else {
            throw BYOKConnectError.invalidTokenResponse
        }

        return token
    }

    func connectAssemblyAI() async throws -> BYOKTokenResponse {
        let pending = try makeAuthorization(provider: "assemblyai", scope: "key:assemblyai")
        let callbackURL = try await authenticate(url: pending.authorizationURL)
        let code = try authorizationCode(from: callbackURL, expectedState: pending.state)
        let token = try await exchangeCode(code, pending: pending)

        guard token.provider == "assemblyai" else {
            throw BYOKConnectError.unexpectedProvider
        }
        guard token.tokenType == "raw_api_key", !token.apiKey.isEmpty else {
            throw BYOKConnectError.invalidTokenResponse
        }

        return token
    }

    func connectOllama() async throws -> BYOKTokenResponse {
        let pending = try makeAuthorization(provider: "ollama", scope: "key:ollama")
        let callbackURL = try await authenticate(url: pending.authorizationURL)
        let code = try authorizationCode(from: callbackURL, expectedState: pending.state)
        let token = try await exchangeCode(code, pending: pending)

        guard token.provider == "ollama" else {
            throw BYOKConnectError.unexpectedProvider
        }
        guard token.tokenType == "raw_api_key", !token.apiKey.isEmpty else {
            throw BYOKConnectError.invalidTokenResponse
        }

        return token
    }

    func connectPerplexity() async throws -> BYOKTokenResponse {
        let pending = try makeAuthorization(provider: "perplexity", scope: "key:perplexity")
        let callbackURL = try await authenticate(url: pending.authorizationURL)
        let code = try authorizationCode(from: callbackURL, expectedState: pending.state)
        let token = try await exchangeCode(code, pending: pending)

        guard token.provider == "perplexity" else {
            throw BYOKConnectError.unexpectedProvider
        }
        guard token.tokenType == "raw_api_key", !token.apiKey.isEmpty else {
            throw BYOKConnectError.invalidTokenResponse
        }

        return token
    }

    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        if let activeScene = scenes.first(where: { $0.activationState == .foregroundActive }),
           let keyWindow = activeScene.windows.first(where: { $0.isKeyWindow }) {
            return keyWindow
        }
        if let fallbackWindow = scenes.flatMap(\.windows).first {
            return fallbackWindow
        }
        if let firstScene = scenes.first {
            return UIWindow(windowScene: firstScene)
        }
        preconditionFailure("BYOKConnectService: no UIWindowScene available to present authentication")
    }

    private func makeAuthorization(provider: String, scope: String) throws -> BYOKPendingAuthorization {
        let state = try Self.randomBase64URL(byteCount: Constants.stateByteCount)
        let codeVerifier = try Self.randomBase64URL(byteCount: Constants.codeVerifierByteCount)
        let codeChallenge = Self.sha256Base64URL(codeVerifier)
        let redirectURI = "\(redirectScheme)://\(redirectHost)"

        // Force-unwrap is safe: authorizationBaseURL is a hardcoded literal URL that
        // URLComponents can always decompose successfully.
        var components = URLComponents(url: authorizationBaseURL, resolvingAgainstBaseURL: false)!
        components.queryItems = [
            URLQueryItem(name: "response_type", value: "code"),
            URLQueryItem(name: "client_id", value: clientID),
            URLQueryItem(name: "app_name", value: appName),
            URLQueryItem(name: "redirect_uri", value: redirectURI),
            URLQueryItem(name: "scope", value: scope),
            URLQueryItem(name: "state", value: state),
            URLQueryItem(name: "code_challenge", value: codeChallenge),
            URLQueryItem(name: "code_challenge_method", value: "S256"),
        ]

        guard let authorizationURL = components.url else {
            throw BYOKConnectError.invalidAuthorizationURL
        }

        return BYOKPendingAuthorization(
            provider: provider,
            authorizationURL: authorizationURL,
            redirectURI: redirectURI,
            clientID: clientID,
            state: state,
            codeVerifier: codeVerifier
        )
    }

    private func makeAuthorization(providers: [BYOKProvider]) throws -> BYOKPendingAuthorization {
        let scope = providers.map(\.scope).joined(separator: " ")
        let provider = providers.map(\.rawValue).joined(separator: ",")
        return try makeAuthorization(provider: provider, scope: scope)
    }

    private func authenticate(url: URL) async throws -> URL {
        try await withCheckedThrowingContinuation { continuation in
            let session = ASWebAuthenticationSession(url: url, callbackURLScheme: redirectScheme) { [weak self] callbackURL, error in
                Task { @MainActor in
                    self?.currentSession = nil

                    if let error {
                        if let authError = error as? ASWebAuthenticationSessionError,
                           authError.code == .canceledLogin {
                            continuation.resume(throwing: BYOKConnectError.cancelled)
                            return
                        }
                        continuation.resume(throwing: BYOKConnectError.authenticationFailed)
                        return
                    }

                    guard let callbackURL else {
                        continuation.resume(throwing: BYOKConnectError.invalidCallback)
                        return
                    }
                    continuation.resume(returning: callbackURL)
                }
            }

            session.presentationContextProvider = self
            session.prefersEphemeralWebBrowserSession = false
            currentSession = session

            guard session.start() else {
                currentSession = nil
                continuation.resume(throwing: BYOKConnectError.authenticationFailed)
                return
            }
        }
    }

    private func authorizationCode(from callbackURL: URL, expectedState: String) throws -> String {
        guard callbackURL.scheme == redirectScheme,
              callbackURL.host == redirectHost,
              let components = URLComponents(url: callbackURL, resolvingAgainstBaseURL: false) else {
            throw BYOKConnectError.invalidCallback
        }

        let query = Dictionary(uniqueKeysWithValues: (components.queryItems ?? []).map { ($0.name, $0.value ?? "") })
        if query["state"] != expectedState {
            throw BYOKConnectError.stateMismatch
        }
        if query["error"] == "access_denied" {
            throw BYOKConnectError.accessDenied
        }
        guard let code = query["code"], !code.isEmpty else {
            throw BYOKConnectError.missingCode
        }
        return code
    }

    private func exchangeCode(_ code: String, pending: BYOKPendingAuthorization) async throws -> BYOKTokenResponse {
        var request = URLRequest(url: tokenURL)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.timeoutInterval = Constants.tokenRequestTimeout

        let body = BYOKTokenRequest(
            code: code,
            codeVerifier: pending.codeVerifier,
            clientID: pending.clientID,
            redirectURI: pending.redirectURI
        )
        request.httpBody = try Self.encoder.encode(body)

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse else {
            throw BYOKConnectError.tokenExchangeFailed
        }
        if !(200..<300).contains(http.statusCode) {
            let tokenError = try? Self.decoder.decode(BYOKTokenErrorResponse.self, from: data)
            throw BYOKConnectError.serverRejectedToken(error: tokenError?.error)
        }

        do {
            return try Self.decoder.decode(BYOKTokenResponse.self, from: data)
        } catch {
            throw BYOKConnectError.invalidTokenResponse
        }
    }

    private var clientID: String {
        Bundle.main.bundleIdentifier ?? "com.podcastr.podcastr"
    }

    private var appName: String {
        if let displayName = Bundle.main.object(forInfoDictionaryKey: "CFBundleDisplayName") as? String,
           !displayName.isEmpty {
            return displayName
        }
        return Bundle.main.object(forInfoDictionaryKey: "CFBundleName") as? String ?? "Podcastr"
    }

    private static func randomBase64URL(byteCount: Int) throws -> String {
        var bytes = [UInt8](repeating: 0, count: byteCount)
        let status = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        guard status == errSecSuccess else {
            throw BYOKConnectError.randomGenerationFailed
        }
        return Data(bytes).base64URLEncodedString()
    }

    private static func sha256Base64URL(_ value: String) -> String {
        let digest = SHA256.hash(data: Data(value.utf8))
        return Data(digest).base64URLEncodedString()
    }
}

// DTOs (`BYOKPendingAuthorization`, `BYOKTokenRequest`, `BYOKTokenResponse`,
// `BYOKTokenErrorResponse`, `BYOKConnectError`) and the `Data.base64URLEncodedString`
// helper live in `BYOKModels.swift`.
