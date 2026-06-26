import Foundation

// ── Relay management + NIP-47 wallet operations ───────────────────────────────
// Extracted from KernelBridge.swift to satisfy the 500-LOC ceiling (#962).

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

    @discardableResult
    func publishDmRelayList(relays: [String]) -> DispatchResult {
        dispatchRelayWalletAction("nmp.nip17.publish_relay_list", body: ["relays": relays])
    }

    /// `nmp.nip65.publish_relay_list` — dispatches a kind:10002 NIP-65
    /// relay-list metadata event. Swift forwards the kernel-authored
    /// `AppRelay` role string verbatim; Rust normalizes composite roles
    /// like `"both,indexer"` and skips indexer-only rows when building the
    /// kind:10002 tags.
    @discardableResult
    func publishRelayList(relays: [AppRelay]) -> DispatchResult {
        return dispatchRelayWalletAction(
            "nmp.nip65.publish_relay_list",
            body: ["relays": relays.map { ["url": $0.url, "role": $0.role] }])
    }

    // ── NIP-47 Wallet Connect ─────────────────────────────────────────────
    //
    // #1607: the bespoke nmp_app_wallet_* FFI symbols were deleted (D11 —
    // one action door). All three operations now route through
    // nmp_app_dispatch_action. The bolt11 double-tap guard lives inside
    // WalletPayInvoiceModule (nmp-nip47); a duplicate tap returns a
    // Conflict rejection which is surfaced as a DispatchResult.failure below
    // rather than a silent no-op. The caller (WalletViewModel) may check
    // the DispatchResult and choose to present user-visible feedback.

    /// Connect a NIP-47 wallet. Errors (invalid URI scheme) arrive as
    /// `DispatchResult.failure`; the kernel also emits a `ShowToast` actor
    /// command that surfaces through `last_error_toast` in the snapshot.
    @discardableResult
    func walletConnect(uri: String) -> DispatchResult {
        dispatchRelayWalletAction("nmp.wallet.connect",
                                  body: ["Connect": ["uri": uri]])
    }

    /// Disconnect the current NIP-47 wallet (fire-and-forget).
    @discardableResult
    func walletDisconnect() -> DispatchResult {
        dispatchRelayWalletRaw("nmp.wallet.disconnect", bodyJson: "\"Disconnect\"")
    }

    /// Pay a Lightning invoice. Returns a `DispatchResult` with the
    /// correlation_id so the caller can drive a payment-progress spinner.
    /// A duplicate bolt11 tap within the TTL window returns
    /// `DispatchResult.failure("payment already in progress…")`.
    @discardableResult
    func walletPayInvoice(bolt11: String, amountMsats: UInt64?) -> DispatchResult {
        var body: [String: Any] = ["bolt11": bolt11]
        if let amount = amountMsats {
            body["amount_msats"] = amount
        } else {
            body["amount_msats"] = NSNull()
        }
        return dispatchRelayWalletAction("nmp.wallet.pay_invoice",
                                         body: ["PayInvoice": body])
    }

    @discardableResult
    private func dispatchRelayWalletAction(
        _ namespace: String,
        body: [String: Any]
    ) -> DispatchResult {
        guard let data = try? JSONSerialization.data(withJSONObject: body),
              let json = String(data: data, encoding: .utf8) else {
            return .failure("failed to serialize action body")
        }
        return dispatchRelayWalletRaw(namespace, bodyJson: json)
    }

    @discardableResult
    private func dispatchRelayWalletRaw(_ namespace: String, bodyJson: String) -> DispatchResult {
        let envelope: String? = bodyJson.withCString { jsonPtr in
            namespace.withCString { nsPtr in
                guard let ptr = nmp_app_chirp_dispatch_action_bytes(raw, nsPtr, jsonPtr) else {
                    return nil
                }
                defer { nmp_free_string(ptr) }
                return String(cString: ptr)
            }
        }
        guard let envelope else {
            return .failure("dispatch returned a null envelope")
        }
        return DispatchResult.parse(envelope: envelope)
    }
}
