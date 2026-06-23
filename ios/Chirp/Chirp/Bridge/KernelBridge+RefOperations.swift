import Foundation

// ── Reference resolution / event claim operations ────────────────────────────
// Extracted from KernelBridge.swift to satisfy the 500-LOC ceiling (#962).

extension KernelHandle {
    /// ADR-0063 Lane E (#1671) — unified, origin-blind reference resolution.
    /// Supersedes `claimProfile` / `claimEvent`: registers (or upgrades) this
    /// `consumerID`'s interest in `(namespace, key)` at the requested `shape`
    /// and `liveness`. The kernel surfaces the resolved entity in the matching
    /// keyed projection (`refs.profile` / `refs.event`) keyed by `key`, which
    /// the `keyedRefCache` consumes on the next frame. Fire-and-forget.
    func resolveRef(
        namespace: RefNamespace,
        key: String,
        consumerID: String,
        shape: RefShape,
        liveness: RefLiveness
    ) {
        key.withCString { keyPtr in
            consumerID.withCString { cidPtr in
                nmp_app_resolve_ref(
                    raw, namespace.rawValue, keyPtr, cidPtr,
                    shape.rawValue, liveness.rawValue)
            }
        }
    }

    /// ADR-0063 Lane E (#1671) — release a reference registered via
    /// `resolveRef`. Pass the SAME `namespace` / `key` / `consumerID`.
    func releaseRef(namespace: RefNamespace, key: String, consumerID: String) {
        key.withCString { keyPtr in
            consumerID.withCString { cidPtr in
                nmp_app_release_ref(raw, namespace.rawValue, keyPtr, cidPtr)
            }
        }
    }

    /// ADR-0032 / V-115: bech32-encode a hex pubkey as `npub1…` on the shell
    /// side. Projections no longer carry pre-encoded npub strings; shells call
    /// this when they need the bech32 form (copy-to-clipboard, share sheet).
    /// Returns `nil` if the C function fails (e.g. invalid key).
    func encodeProfile(pubkey: String) -> String? {
        pubkey.withCString { pkPtr -> String? in
            guard let ptr = nmp_app_encode_profile(raw, pkPtr) else { return nil }
            defer { nmp_free_string(ptr) }
            return String(cString: ptr)
        }
    }

    // #1726: claimEvent / releaseEvent DELETED.
    // Use resolveRef(namespace: .event, key: decodedEventId, ...) instead.
    // To decode a `nostr:` URI to an event key, call nmp_nip21_decode_uri and
    // extract the event_id field from the JSON result, then pass it as `key`.

    /// #1726 — Decode a `nostr:` URI and resolve the embedded event ref via the
    /// unified ref-resolution seam. Supersedes the deleted `claimEvent(uri:…)`.
    ///
    /// The URI is decoded via `nmp_nip21_decode_uri`; on success the extracted
    /// event key is forwarded to `resolveRef(namespace: .event, …)`. On decode
    /// failure (or if the URI does not resolve to an event) this is a silent
    /// no-op (D6).
    func claimEventUri(uri: String, consumerID: String, force: Bool = false) {
        // Decode the nostr: URI to extract the event key.
        guard let jsonStr = uri.withCString({ ptr -> String? in
            guard let cResult = nmp_nip21_decode_uri(ptr) else { return nil }
            defer { nmp_free_string(cResult) }
            return String(cString: cResult)
        }) else { return }
        guard let jsonData = jsonStr.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: jsonData) as? [String: Any],
              let ok = obj["ok"] as? Bool, ok,
              let eventId = obj["event_id"] as? String
        else { return }
        // Use CacheOk (0) for background, Live (1) for explicit navigation.
        let liveness: RefLiveness = force ? .live : .cacheOk
        resolveRef(namespace: .event, key: eventId, consumerID: consumerID,
                   shape: .eventEmbed, liveness: liveness)
    }

    /// #1726 — Release a previously-claimed event ref (mirror of `claimEventUri`).
    func releaseEventUri(uri: String, consumerID: String) {
        guard let jsonStr = uri.withCString({ ptr -> String? in
            guard let cResult = nmp_nip21_decode_uri(ptr) else { return nil }
            defer { nmp_free_string(cResult) }
            return String(cString: cResult)
        }) else { return }
        guard let jsonData = jsonStr.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: jsonData) as? [String: Any],
              let ok = obj["ok"] as? Bool, ok,
              let eventId = obj["event_id"] as? String
        else { return }
        releaseRef(namespace: .event, key: eventId, consumerID: consumerID)
    }
}
