import Foundation
import Security

/// A thin, synchronous wrapper around the iOS Keychain for storing and
/// retrieving UTF-8 string values. Each item is keyed by a `(service, account)`
/// pair, which maps to the Generic Password Keychain item class.
///
/// All methods throw `KeychainStoreError` on failure so callers can surface
/// errors rather than silently dropping Keychain operations.
enum KeychainStore {
    /// Persists `value` in the Keychain under the given `service` and `account`.
    ///
    /// Overwrites any existing item with the same key. The item is stored with
    /// `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` so it is only readable
    /// while the device is unlocked and is never migrated to a new device.
    ///
    /// - Throws: `KeychainStoreError.unhandledStatus` if `SecItemAdd` fails.
    static func saveString(_ value: String, service: String, account: String) throws {
        let data = Data(value.utf8)
        var query = baseQuery(service: service, account: account)
        SecItemDelete(query as CFDictionary)

        query[kSecValueData as String] = data
        query[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly

        let status = SecItemAdd(query as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw KeychainStoreError.unhandledStatus(status)
        }
    }

    /// Reads the string stored under the given `service` and `account`.
    ///
    /// - Returns: The stored value, or `nil` if no item exists for that key.
    /// - Throws: `KeychainStoreError.unhandledStatus` if the Keychain query fails,
    ///   or `KeychainStoreError.invalidData` if the stored bytes cannot be decoded
    ///   as UTF-8.
    static func readString(service: String, account: String) throws -> String? {
        var query = baseQuery(service: service, account: account)
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        query[kSecReturnData as String] = true

        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)

        if status == errSecItemNotFound {
            return nil
        }
        guard status == errSecSuccess else {
            throw KeychainStoreError.unhandledStatus(status)
        }
        guard let data = result as? Data else {
            throw KeychainStoreError.invalidData
        }
        return String(data: data, encoding: .utf8)
    }

    /// Removes the item stored under the given `service` and `account`.
    ///
    /// Succeeds silently if no item exists for the key (idempotent).
    ///
    /// - Throws: `KeychainStoreError.unhandledStatus` if `SecItemDelete` fails
    ///   for any reason other than item-not-found.
    static func deleteString(service: String, account: String) throws {
        let query = baseQuery(service: service, account: account)
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw KeychainStoreError.unhandledStatus(status)
        }
    }

    private static func baseQuery(service: String, account: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
    }
}

enum KeychainStoreError: LocalizedError {
    case invalidData
    case unhandledStatus(OSStatus)

    var errorDescription: String? {
        switch self {
        case .invalidData:
            "Keychain item could not be decoded."
        case .unhandledStatus:
            "Keychain operation failed."
        }
    }
}

