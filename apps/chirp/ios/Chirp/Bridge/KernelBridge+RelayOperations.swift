import Foundation

// ── Relay management + NIP-47 wallet operations ───────────────────────────────
// Extracted from KernelBridge.swift to satisfy the 500-LOC ceiling (#962).
// M14-1 (#2145): all write verbs use GeneratedActionBuilders bytes — no
// namespace strings or JSON assembly in host code.

extension KernelHandle {
    func addRelay(url: String, role: String) {
        url.withCString { uPtr in
            role.withCString { rPtr in
                nmp_app_add_relay(raw, uPtr, rPtr)
            }
        }
    }

    /// Seed the Chirp reference relay set. The default relay list lives in Rust
    /// (`nmp-chirp-config`, surfaced via `nmp_app_chirp_seed_default_relays`),
    /// not in Swift (D7 / thin-shell) — the shell no longer hardcodes URLs.
    /// Returns `false` only on a null app handle.
    @discardableResult
    func seedDefaultRelays() -> Bool {
        nmp_app_chirp_seed_default_relays(raw)
    }

    /// Seed relays from a `[["url","role"],…]` JSON array (the `NMP_TEST_RELAYS`
    /// override shape). Parsing/validation live in Rust
    /// (`nmp_app_chirp_seed_relays_from_json`); returns `false` when the JSON is
    /// malformed or empty so the caller can fall back to `seedDefaultRelays()`.
    func seedRelays(fromJSON json: String) -> Bool {
        json.withCString { nmp_app_chirp_seed_relays_from_json(raw, $0) }
    }

    func removeRelay(url: String) {
        url.withCString { nmp_app_remove_relay(raw, $0) }
    }

    /// Publish the NIP-17 DM relay list (kind:10050) via the typed FlatBuffers
    /// byte builder (M14-1 / #2145).
    @discardableResult
    func publishDmRelayList(relays: [String]) -> DispatchResult {
        let id = UUID().uuidString
        let bytes = GeneratedActionBuilders.publishDmRelayList(correlationId: id, relays: relays)
        return dispatchBytes(bytes)
    }

    /// `nmp.nip65.publish_relay_list` — dispatches a kind:10002 NIP-65
    /// relay-list metadata event via the typed FlatBuffers byte builder
    /// (M14-1 / #2145). The generated builder maps role strings to the
    /// `RelayMarker` ubyte; Rust normalises composite roles and skips
    /// indexer-only rows when building the kind:10002 tags.
    @discardableResult
    func publishRelayList(relays: [AppRelay]) -> DispatchResult {
        let id = UUID().uuidString
        let entries = relays.map { (url: $0.url, role: $0.role) }
        let bytes = GeneratedActionBuilders.publishRelayList(correlationId: id, relays: entries)
        return dispatchBytes(bytes)
    }

    /// Block a relay via the typed FlatBuffers byte builder (M14-1 / #2145).
    @discardableResult
    func blockRelay(url: String, accountPubkey: String) -> DispatchResult {
        let id = UUID().uuidString
        let bytes = GeneratedActionBuilders.blockRelay(
            correlationId: id, url: url, accountPubkey: accountPubkey)
        return dispatchBytes(bytes)
    }

    /// Unblock a relay via the typed FlatBuffers byte builder (M14-1 / #2145).
    @discardableResult
    func unblockRelay(url: String, accountPubkey: String) -> DispatchResult {
        let id = UUID().uuidString
        let bytes = GeneratedActionBuilders.unblockRelay(
            correlationId: id, url: url, accountPubkey: accountPubkey)
        return dispatchBytes(bytes)
    }

    // ── NIP-47 Wallet Connect ─────────────────────────────────────────────
    //
    // M14-1 (#2145): all three operations use GeneratedActionBuilders bytes
    // dispatched via `nmp_app_dispatch_action_bytes` (the generic byte doorway).
    // The bolt11 double-tap guard lives inside WalletPayInvoiceModule (nmp-nip47);
    // a duplicate tap returns a Conflict rejection surfaced as DispatchResult.failure.

    /// Connect a NIP-47 wallet. Errors (invalid URI scheme) arrive as
    /// `DispatchResult.failure`; the kernel also emits a `ShowToast` actor
    /// command that surfaces through `last_error_toast` in the snapshot.
    @discardableResult
    func walletConnect(uri: String) -> DispatchResult {
        let id = UUID().uuidString
        let bytes = GeneratedActionBuilders.walletConnect(correlationId: id, uri: uri)
        return dispatchBytes(bytes)
    }

    /// Disconnect the current NIP-47 wallet (fire-and-forget).
    @discardableResult
    func walletDisconnect() -> DispatchResult {
        let id = UUID().uuidString
        let bytes = GeneratedActionBuilders.walletDisconnect(correlationId: id)
        return dispatchBytes(bytes)
    }

    /// Pay a Lightning invoice. Returns a `DispatchResult` with the
    /// correlation_id so the caller can drive a payment-progress spinner.
    /// A duplicate bolt11 tap within the TTL window returns
    /// `DispatchResult.failure("payment already in progress…")`.
    @discardableResult
    func walletPayInvoice(bolt11: String, amountMsats: UInt64?) -> DispatchResult {
        let id = UUID().uuidString
        let bytes = GeneratedActionBuilders.walletPayInvoice(
            correlationId: id, bolt11: bolt11, amountMsats: amountMsats)
        return dispatchBytes(bytes)
    }

    /// Dispatch pre-built `DispatchEnvelope` FlatBuffers bytes through the
    /// generic typed byte doorway `nmp_app_dispatch_action_bytes` (M14-1 / #2145).
    /// D6: returns `.failure` on a null result (dead handle or internal error).
    func dispatchBytes(_ bytes: [UInt8]) -> DispatchResult {
        let envelope: String? = bytes.withUnsafeBytes { rawBufPtr -> String? in
            guard let basePtr = rawBufPtr.bindMemory(to: UInt8.self).baseAddress else {
                return nil
            }
            guard let resPtr = nmp_app_dispatch_action_bytes(raw, basePtr, UInt(rawBufPtr.count)) else {
                return nil
            }
            defer { nmp_free_string(resPtr) }
            return String(cString: resPtr)
        }
        guard let envelope else {
            return .failure("dispatch returned a null envelope")
        }
        return DispatchResult.parse(envelope: envelope)
    }
}
