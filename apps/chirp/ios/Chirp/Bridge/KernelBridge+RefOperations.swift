import Foundation

// ── Reference resolution / event claim operations ────────────────────────────
// Extracted from KernelBridge.swift to satisfy the 500-LOC ceiling (#962).

extension KernelHandle {
    /// ADR-0063 Lane E (#1671) — typed profile reference resolution.
    /// Registers (or upgrades) this `consumerID`'s interest in `key` using one
    /// of the supported profile adapters. Unsupported shape/liveness pairs fail
    /// closed at the bridge rather than crossing the raw integer ABI.
    func resolveProfile(key: String, consumerID: String, shape: RefShape, liveness: RefLiveness) {
        key.withCString { keyPtr in
            consumerID.withCString { cidPtr in
                switch (shape, liveness) {
                case (.profileRef, .cacheOk):
                    nmp_app_resolve_profile_ref(raw, keyPtr, cidPtr)
                case (.profileCard, .live):
                    nmp_app_resolve_profile_card_live(raw, keyPtr, cidPtr)
                default:
                    break
                }
            }
        }
    }

    /// ADR-0063 Lane E (#1671) — release a profile reference registered via
    /// `resolveProfile`. Pass the SAME `key` / `consumerID`.
    func releaseProfile(key: String, consumerID: String) {
        key.withCString { keyPtr in
            consumerID.withCString { cidPtr in
                nmp_app_release_profile_ref(raw, keyPtr, cidPtr)
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

    private struct EventRefFromUri {
        let key: String
        let metadataJson: String
    }

    // #1726 — nostr: URI → canonical event key helper.
    // Returns the raw key plus decoded metadata the kernel resolver expects:
    //   - nevent / note  → the hex event_id
    //   - naddr          → the canonical coordinate string "kind:pubkey:identifier"
    // Returns nil on any decode failure (D6: silent no-op).
    private func eventRefFromUri(_ uri: String) -> EventRefFromUri? {
        guard let jsonStr = uri.withCString({ ptr -> String? in
            guard let cResult = nmp_nip21_decode_uri(ptr) else { return nil }
            defer { nmp_free_string(cResult) }
            return String(cString: cResult)
        }) else { return nil }
        guard let jsonData = jsonStr.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: jsonData) as? [String: Any],
              let ok = obj["ok"] as? Bool, ok
        else { return nil }
        let key: String
        switch obj["target"] as? String {
        case "event":
            guard let eventId = obj["event_id"] as? String else { return nil }
            key = eventId
        case "address":
            guard let kind = obj["kind"] as? NSNumber,
                  let pubkey = obj["pubkey"] as? String,
                  let identifier = obj["identifier"] as? String
            else { return nil }
            key = "\(kind.uint32Value):\(pubkey):\(identifier)"
        default:
            return nil
        }
        var metadata: [String: Any] = ["hints": obj["relays"] as? [String] ?? []]
        if let author = obj["author"] as? String {
            metadata["author"] = author
        }
        if let kind = obj["kind"] as? NSNumber {
            metadata["kind"] = kind.uint32Value
        }
        guard let data = try? JSONSerialization.data(withJSONObject: metadata),
              let metadataJson = String(data: data, encoding: .utf8)
        else { return nil }
        return EventRefFromUri(key: key, metadataJson: metadataJson)
    }

    /// #1726 — Decode a `nostr:` URI and resolve the embedded event ref via the
    /// typed event-embed adapter.
    ///
    /// Handles `nevent`/`note` URIs (hex event_id key) and `naddr` URIs
    /// (canonical `kind:pubkey:identifier` coordinate key). On decode failure or
    /// a non-event URI this is a silent no-op (D6).
    func claimEventUri(uri: String, consumerID: String, force: Bool = false) {
        guard let eventRef = eventRefFromUri(uri) else { return }
        eventRef.key.withCString { keyPtr in
            consumerID.withCString { cidPtr in
                eventRef.metadataJson.withCString { metadataPtr in
                    if force {
                        nmp_app_resolve_event_embed_live_with_metadata(
                            raw, keyPtr, cidPtr, metadataPtr)
                    } else {
                        nmp_app_resolve_event_embed_with_metadata(
                            raw, keyPtr, cidPtr, metadataPtr)
                    }
                }
            }
        }
    }

    /// #1726 — Release a previously-claimed event ref (mirror of `claimEventUri`).
    func releaseEventUri(uri: String, consumerID: String) {
        guard let eventRef = eventRefFromUri(uri) else { return }
        eventRef.key.withCString { keyPtr in
            consumerID.withCString { cidPtr in
                nmp_app_release_event_ref(raw, keyPtr, cidPtr)
            }
        }
    }
}
