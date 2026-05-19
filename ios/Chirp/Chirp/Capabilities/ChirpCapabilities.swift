import Foundation

/// Capability injection point for Chirp.
///
/// The kernel grants the app a set of capability *sockets*; the app supplies
/// the platform implementation. This holder is the one place those
/// implementations are constructed and started, mirroring the thin-bridge
/// pattern in `Bridge/KernelBridge.swift`.
///
/// Currently it owns the `KeychainCapability` (at-rest secret storage). The
/// kernel-side FFI socket that routes `CapabilityRequest`s here does not yet
/// exist (the keyring `KeyringCapability` Rust contract + `nmp_app_*`
/// capability callback are unbuilt — tracked in
/// `docs/perf/pending-user-decisions.md` PD-019, and in `README.md` row
/// "Keychain at-rest secret storage"). Until that lands, the Onboarding flow
/// (also deferred — README "Onboarding (paste nsec / bunker / create)") calls
/// `persistImportedSecret(accountID:secret:)` directly; when the FFI socket
/// graduates, the kernel routes through `keyring.handleJSON(_:)` instead and
/// this direct method becomes a thin shim over the same code path.
final class ChirpCapabilities {
    let keyring: KeychainCapability

    init(keyring: KeychainCapability = KeychainCapability()) {
        self.keyring = keyring
    }

    /// Idempotent: start all owned capabilities. Safe to call on every app
    /// foreground.
    func start() {
        keyring.start()
    }

    /// Idempotent: mark capabilities inactive. Does not erase stored secrets.
    func stop() {
        keyring.stop()
    }

    /// Onboarding helper: persist an imported `nsec`/key for `accountID`.
    ///
    /// Routes through the same envelope path the kernel will use, so behavior
    /// is identical pre- and post-FFI-wireup. Returns `true` iff the Keychain
    /// reported success. Never throws (D6).
    @discardableResult
    func persistImportedSecret(accountID: String, secret: String) -> Bool {
        let request = CapabilityRequest(
            namespace: KeychainCapability.namespace,
            correlationID: UUID().uuidString,
            payloadJSON: Self.storePayload(accountID: accountID, secret: secret))
        let envelope = keyring.handle(request)
        return envelope.resultJSON.contains("\"status\":\"ok\"")
    }

    /// Retrieve a previously-persisted secret from the keychain.
    /// Returns `nil` if the item is absent or the Keychain reports an error.
    func retrieveSecret(accountID: String) -> String? {
        let request = CapabilityRequest(
            namespace: KeychainCapability.namespace,
            correlationID: UUID().uuidString,
            payloadJSON: Self.retrievePayload(accountID: accountID))
        let envelope = keyring.handle(request)
        guard envelope.resultJSON.contains("\"status\":\"ok\"") else { return nil }
        guard let data = envelope.resultJSON.data(using: .utf8),
              let result = try? JSONDecoder().decode(KeyringResult.self, from: data)
        else { return nil }
        return result.secret
    }

    private static func storePayload(accountID: String, secret: String) -> String {
        let payload: [String: String] = [
            "op": "store",
            "account_id": accountID,
            "secret": secret,
        ]
        guard
            let data = try? JSONSerialization.data(withJSONObject: payload),
            let json = String(data: data, encoding: .utf8)
        else {
            return "{}"
        }
        return json
    }

    private static func retrievePayload(accountID: String) -> String {
        let payload: [String: String] = [
            "op": "retrieve",
            "account_id": accountID,
        ]
        guard
            let data = try? JSONSerialization.data(withJSONObject: payload),
            let json = String(data: data, encoding: .utf8)
        else {
            return "{}"
        }
        return json
    }
}
