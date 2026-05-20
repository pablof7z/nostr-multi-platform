import Foundation

/// Capability injection point for Chirp.
///
/// The kernel grants the app a set of capability *sockets*; the app supplies
/// the platform implementation. This holder is the one place those
/// implementations are constructed and started, mirroring the thin-bridge
/// pattern in `Bridge/KernelBridge.swift`.
///
/// Currently it owns the `KeychainCapability` (at-rest secret storage). Rust
/// decides when to store, recall, or forget; Swift only executes the keyring
/// request and reports the raw result.
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
}
