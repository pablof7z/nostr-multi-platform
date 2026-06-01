import Foundation
import Security

// MARK: - Capability envelope contract
//
// Mirrors `crates/nmp-core/src/substrate/capability.rs`:
//
//   struct CapabilityRequest  { namespace, correlation_id, payload_json }
//   struct CapabilityEnvelope { namespace, correlation_id, result_json }
//
// Doctrine (docs/product-spec/overview-and-dx.md §1.5):
//   D6 — errors never cross the boundary as exceptions. Every failure path
//        returns a populated `result_json` describing the outcome; this type
//        never `throw`s across `handle(_:)`.
//   D7 — a capability reports and executes; it never decides policy. This
//        capability performs the exact Keychain operation the kernel asks
//        for and reports the OS result. It does not decide *whether* a key
//        should be stored, *which* key is active, or *when* to evict — those
//        are kernel/IdentityModule policy decisions.

/// Wire-shape of an incoming capability request. Decoded from the kernel's
/// `CapabilityRequest`. `payloadJSON` is an opaque, capability-private blob.
struct CapabilityRequest: Decodable {
    let namespace: String
    let correlationID: String
    let payloadJSON: String

    enum CodingKeys: String, CodingKey {
        case namespace
        case correlationID = "correlation_id"
        case payloadJSON = "payload_json"
    }
}

/// Wire-shape of the result handed back to the kernel. Encodes to the
/// kernel's `CapabilityEnvelope`.
struct CapabilityEnvelope: Encodable, Equatable {
    let namespace: String
    let correlationID: String
    let resultJSON: String

    enum CodingKeys: String, CodingKey {
        case namespace
        case correlationID = "correlation_id"
        case resultJSON = "result_json"
    }
}

// MARK: - Keyring payload / result vocabulary

/// Capability-private request payload (the decoded `payload_json`).
///
/// The kernel's keyring capability contract is not yet defined in the Rust
/// tree (filed: the kernel-side `KeyringCapability` Request/Result enum +
/// IdentityModule wiring + the FFI/actor socket — see
/// `docs/perf/pending-user-decisions.md` PD-019). This Swift vocabulary is
/// the minimal, self-contained shape the iOS side needs and is the shape the
/// kernel side should converge on: a key/value secret store keyed by an
/// account-scoped account identifier.
enum KeyringRequest: Decodable, Equatable {
    /// Persist `secret` under `accountID`. Overwrites any existing value.
    case store(accountID: String, secret: String)
    /// Read the secret stored under `accountID`.
    case retrieve(accountID: String)
    /// Remove the secret stored under `accountID` (no-op if absent).
    case delete(accountID: String)

    private enum CodingKeys: String, CodingKey {
        case op
        case accountID = "account_id"
        case secret
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let op = try c.decode(String.self, forKey: .op)
        let accountID = try c.decode(String.self, forKey: .accountID)
        switch op {
        case "store":
            self = .store(accountID: accountID, secret: try c.decode(String.self, forKey: .secret))
        case "retrieve":
            self = .retrieve(accountID: accountID)
        case "delete":
            self = .delete(accountID: accountID)
        default:
            throw DecodingError.dataCorruptedError(
                forKey: .op, in: c, debugDescription: "unknown keyring op: \(op)")
        }
    }
}

/// Capability-private result payload (the encoded `result_json`).
///
/// Note there is no error *exception*: a failure is data (`status == "error"`
/// with an `os_status` code), satisfying D6.
struct KeyringResult: Codable, Equatable {
    let status: String          // "ok" | "not_found" | "error"
    let secret: String?         // populated only for a successful retrieve
    let osStatus: Int32?        // raw OSStatus for diagnostics; nil on success

    enum CodingKeys: String, CodingKey {
        case status
        case secret
        case osStatus = "os_status"
    }

    static func ok(secret: String? = nil) -> KeyringResult {
        KeyringResult(status: "ok", secret: secret, osStatus: nil)
    }
    static let notFound = KeyringResult(status: "not_found", secret: nil, osStatus: nil)
    static func error(_ os: OSStatus) -> KeyringResult {
        KeyringResult(status: "error", secret: nil, osStatus: Int32(os))
    }
}

/// Swift-side, typed outcome of a secret retrieval.
///
/// The keychain retrieve operation has three genuinely distinct outcomes —
/// the key was found, the key was never stored, or the Keychain itself
/// failed. Collapsing these into `String?` (the prior `retrieveSecret`
/// return type) makes "not stored" and "Keychain error" indistinguishable
/// to callers, which matters: the former is a normal signed-out state, the
/// latter is a fault worth surfacing. This enum keeps them apart.
enum SecretLookup: Equatable {
    /// The secret was found; carries its plaintext value.
    case found(String)
    /// No secret is stored for the requested account (a legitimate state).
    case notFound
    /// The Keychain reported a failure; carries the raw `OSStatus`.
    case error(OSStatus)
}

// MARK: - Keychain-backed capability

/// iOS Keychain Services implementation of the keyring capability.
///
/// Secrets are stored as `kSecClassGenericPassword` items, scoped to this
/// app's keychain access group, accessible only
/// `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` (never synced to iCloud,
/// never restored to a different device — appropriate for a Nostr `nsec`).
final class KeychainCapability {
    static let namespace = "nmp.keyring.capability"

    /// `kSecAttrService` value — namespaces our items within the keychain.
    private let service: String
    private var started = false

    init(service: String = "com.example.NmpPulse.keyring") {
        self.service = service
    }

    // MARK: Lifecycle (idempotent)

    /// Idempotent. Calling repeatedly is a no-op; the Keychain itself is the
    /// durable backing store so there is nothing to (re)allocate.
    func start() {
        started = true
    }

    /// Idempotent. Does NOT erase stored secrets — stopping the capability is
    /// not a policy decision to forget keys (D7). It only marks the handler
    /// inactive so late requests are rejected as data, not crashes.
    func stop() {
        started = false
    }

    var isStarted: Bool { started }

    // MARK: Envelope handling (never throws — D6)

    /// Decode → execute → encode. Any failure (malformed request, capability
    /// stopped, Keychain error) is returned inside the envelope's
    /// `result_json`, never raised. Returns a best-effort error envelope if
    /// even the result cannot be encoded; callers never see a thrown error.
    func handle(_ request: CapabilityRequest) -> CapabilityEnvelope {
        let result = process(request)
        let resultJSON = Self.encode(result)
            ?? Self.encode(KeyringResult.error(errSecParam))
            ?? "{\"status\":\"error\"}"
        return CapabilityEnvelope(
            namespace: Self.namespace,
            correlationID: request.correlationID,
            resultJSON: resultJSON)
    }

    /// Convenience entry point for FFI bridges that hand us the raw kernel
    /// `CapabilityRequest` JSON and want raw `CapabilityEnvelope` JSON back.
    /// Honors D6 end to end: malformed input yields an error envelope string.
    func handleJSON(_ requestJSON: String) -> String {
        guard
            let data = requestJSON.data(using: .utf8),
            let request = try? JSONDecoder().decode(CapabilityRequest.self, from: data)
        else {
            let env = CapabilityEnvelope(
                namespace: Self.namespace,
                correlationID: "",
                resultJSON: Self.encode(KeyringResult.error(errSecParam)) ?? "{\"status\":\"error\"}")
            return Self.encode(env) ?? "{}"
        }
        return Self.encode(handle(request)) ?? "{}"
    }

    // MARK: - Internals

    private func process(_ request: CapabilityRequest) -> KeyringResult {
        guard started else { return .error(errSecNotAvailable) }
        guard
            let payload = request.payloadJSON.data(using: .utf8),
            let keyringRequest = try? JSONDecoder().decode(KeyringRequest.self, from: payload)
        else {
            return .error(errSecParam)
        }

        switch keyringRequest {
        case let .store(accountID, secret):
            return store(accountID: accountID, secret: secret)
        case let .retrieve(accountID):
            return retrieve(accountID: accountID)
        case let .delete(accountID):
            return delete(accountID: accountID)
        }
    }

    private func baseQuery(_ accountID: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: accountID,
        ]
    }

    private func store(accountID: String, secret: String) -> KeyringResult {
        guard let secretData = secret.data(using: .utf8) else { return .error(errSecParam) }

        // Delete-then-add gives deterministic overwrite semantics across iOS
        // versions (SecItemUpdate's attribute matching is fiddly).
        SecItemDelete(baseQuery(accountID) as CFDictionary)

        var attrs = baseQuery(accountID)
        attrs[kSecValueData as String] = secretData
        attrs[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly

        let status = SecItemAdd(attrs as CFDictionary, nil)
        return status == errSecSuccess ? .ok() : .error(status)
    }

    private func retrieve(accountID: String) -> KeyringResult {
        var query = baseQuery(accountID)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne

        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        switch status {
        case errSecSuccess:
            guard
                let data = item as? Data,
                let secret = String(data: data, encoding: .utf8)
            else {
                return .error(errSecDecode)
            }
            return .ok(secret: secret)
        case errSecItemNotFound:
            return .notFound
        default:
            return .error(status)
        }
    }

    private func delete(accountID: String) -> KeyringResult {
        let status = SecItemDelete(baseQuery(accountID) as CFDictionary)
        // Absent key is a successful no-op, not an error (idempotent delete).
        return (status == errSecSuccess || status == errSecItemNotFound)
            ? .ok()
            : .error(status)
    }

    private static func encode<T: Encodable>(_ value: T) -> String? {
        guard let data = try? JSONEncoder().encode(value) else { return nil }
        return String(data: data, encoding: .utf8)
    }
}
