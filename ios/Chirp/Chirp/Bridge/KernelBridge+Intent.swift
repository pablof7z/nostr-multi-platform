import Foundation

/// Input-intent resolver (#1804) C-ABI passthroughs — the omnibox brain.
///
/// THIN SHELL: every method here is a verbatim `withCString` forward to a
/// `nmp_app_*` symbol that returns bounded JSON, decoded into a typed Swift
/// value via `Codable`. There is ZERO classification or routing logic on the
/// Swift side: the kernel decides whether one untyped input is a direct ref, a
/// NIP-05 identifier, a relay URL, free-text search, or a refusal. Swift only
/// renders / navigates on the typed result.
///
/// Two symbols are bridged:
///   * `nmp_app_intent_dispatch` — classify + ACT on the top candidate through
///     NMP's own seams (DirectRef → open-uri, TextQuery → a kernel search
///     session keyed by the session id we pass, Nip05 → the async reverse-lookup
///     worker). Returns the chosen candidate (or the rejection) so the host can
///     drive navigation.
///   * `nmp_nip21_decode_uri` — STATELESS typed decode of a `DirectRef` URI into
///     its target (profile / event / address). This is how a ref candidate maps
///     to a `ChirpRoute`: a typed decode, never a Swift parse.
///
/// Kept in this extension file (not inlined into `KernelBridge.swift`) so the
/// omnibox feature is self-contained and the host god-files stay at their
/// grandfathered size baseline.
extension KernelHandle {
    /// Classify + dispatch one untyped omnibox input through the resolver.
    ///
    /// `sessionID` keys the kernel search session opened when the top candidate
    /// is free text (`TextQuery`); pass the `SearchController`'s session id so
    /// its reactive `N50S` pull surfaces the results. It is ignored for every
    /// other candidate class.
    ///
    /// Returns the typed `IntentDispatchOutcome`, or `nil` when the C call
    /// fails / returns unparseable JSON (treated by the caller as a no-op).
    func dispatchIntent(input: String, scopes: [IntentScope], sessionID: String) -> IntentDispatchOutcome? {
        let request = IntentRequest(input: input, scopes: scopes, textTargets: .userPreferred)
        guard let requestJSON = request.jsonString() else { return nil }
        return requestJSON.withCString { reqPtr -> IntentDispatchOutcome? in
            sessionID.withCString { sessionPtr -> IntentDispatchOutcome? in
                guard let ptr = nmp_app_intent_dispatch(raw, reqPtr, sessionPtr) else { return nil }
                defer { nmp_free_string(ptr) }
                let json = String(cString: ptr)
                return IntentDispatchOutcome.decode(json: json)
            }
        }
    }

    /// Decode a `DirectRef` `nostr:` URI into its typed navigation target.
    /// Pure passthrough to the stateless `nmp_nip21_decode_uri` symbol — the
    /// kernel owns the bech32/NIP-19 decode; Swift only maps the typed result
    /// onto a `ChirpRoute`. Returns `nil` on a decode error.
    func decodeRefTarget(uri: String) -> DecodedRefTarget? {
        uri.withCString { uriPtr -> DecodedRefTarget? in
            guard let ptr = nmp_nip21_decode_uri(uriPtr) else { return nil }
            defer { nmp_free_string(ptr) }
            return DecodedRefTarget.decode(json: String(cString: ptr))
        }
    }
}
