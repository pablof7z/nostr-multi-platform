import Foundation

/// Input-intent resolver (#1804) C-ABI passthroughs.
///
/// THIN SHELL: every method here is a `withCString` forward to an NMP-owned C
/// symbol returning bounded JSON, decoded into typed Swift DTOs. Classification
/// and routing stay in Rust; Swift renders or navigates from the typed result.
extension KernelHandle {
    /// Classify and dispatch one omnibox input through the resolver.
    ///
    /// `sessionID` keys the kernel search session opened when the top candidate
    /// is free text. It is ignored for every other candidate class.
    func dispatchIntent(input: String, scopes: [IntentScope], sessionID: String) -> IntentDispatchOutcome? {
        let request = IntentRequest(input: input, scopes: scopes, textTargets: .userPreferred)
        guard let requestJSON = request.jsonString() else { return nil }
        return requestJSON.withCString { reqPtr -> IntentDispatchOutcome? in
            sessionID.withCString { sessionPtr -> IntentDispatchOutcome? in
                guard let ptr = nmp_app_intent_dispatch(raw, reqPtr, sessionPtr) else { return nil }
                defer { nmp_free_string(ptr) }
                return IntentDispatchOutcome.decode(json: String(cString: ptr))
            }
        }
    }

    /// Decode a `DirectRef` `nostr:` URI into its typed navigation target.
    func decodeRefTarget(uri: String) -> DecodedRefTarget? {
        uri.withCString { uriPtr -> DecodedRefTarget? in
            guard let ptr = nmp_nip21_decode_uri(uriPtr) else { return nil }
            defer { nmp_free_string(ptr) }
            return DecodedRefTarget.decode(json: String(cString: ptr))
        }
    }
}
